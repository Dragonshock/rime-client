//! `weights.safetensors` 的读取。
//!
//! 权重文件是「8 字节小端表头长度 + JSON 表头 + 裸数据」的老格式。本模块只
//! 解表头、按名字定位区间、读出稠密张量与分组权重，不解释层结构。
//!
//! 两座塔的分组位数不同：`quantization_config.json` 写的是文本 4 位、音频
//! `audio_tower_bits` 8 位，组大小都是 64。位数由调用方传入，本模块不猜。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use mlx_rs::ops::{DequantizeOptions, QuantizationMode};
use mlx_rs::{ops, Array, Dtype};
use voice_tensor::{Conv2d, DType, LayerNorm, Linear};

use super::{from_mlx, MlxError, MlxTensor};

/// 已打开的权重文件。
pub(crate) struct SafeWeights {
    file: File,
    data_base: u64,
    header: String,
}

/// 表头里一条张量的位置。
struct TensorLoc {
    dtype: String,
    shape: Vec<usize>,
    start: u64,
    end: u64,
}

impl SafeWeights {
    /// 打开 `model_dir/weights.safetensors`。
    pub(crate) fn open(model_dir: &str) -> Result<Self, MlxError> {
        let path = Path::new(model_dir).join("weights.safetensors");
        if !path.is_file() {
            return Err(MlxError::new("打不开权重"));
        }
        let mut file = File::open(&path).map_err(|_| MlxError::new("打不开权重"))?;
        let mut len_buf = [0u8; 8];
        file.read_exact(&mut len_buf)
            .map_err(|_| MlxError::new("打不开权重"))?;
        let header_len = u64::from_le_bytes(len_buf);
        if header_len > 64 * 1024 * 1024 {
            return Err(MlxError::new("权重头过大"));
        }
        let mut header_bytes = vec![0u8; header_len as usize];
        file.read_exact(&mut header_bytes)
            .map_err(|_| MlxError::new("打不开权重"))?;
        let header =
            String::from_utf8(header_bytes).map_err(|_| MlxError::new("权重头不是文本"))?;
        Ok(Self {
            file,
            data_base: 8 + header_len,
            header,
        })
    }

    /// 表头里有没有这个名字。
    pub(crate) fn has(&self, key: &str) -> bool {
        self.header.contains(&format!("\"{key}\""))
    }

    /// 读稠密张量并转成 `dtype`。卷积、LayerNorm、偏置都走这条。
    pub(crate) fn dense_as(&mut self, key: &str, dtype: DType) -> Result<MlxTensor, MlxError> {
        let loc = self.locate(key)?;
        let bytes = self.read_range(&loc)?;
        let data = match loc.dtype.as_str() {
            "F32" => f32_le(&bytes)?,
            "F16" => f16_le(&bytes)?,
            other => return Err(MlxError::new(format!("{key} 的数据类型 {other} 不是浮点"))),
        };
        let numel = numel(&loc.shape)?;
        if data.len() != numel {
            return Err(MlxError::new(format!("{key} 的元素个数与形状不一致")));
        }
        let tensor = dense_of(&data, &loc.shape, dtype)?;
        tensor.array.eval().map_err(from_mlx)?;
        Ok(tensor)
    }

    /// 卷积层：`<name>.weight` 与 `<name>.bias`，权重形状是 `(out, 3, 3, in)`。
    pub(crate) fn conv2d(
        &mut self,
        name: &str,
        dtype: DType,
    ) -> Result<Conv2d<MlxTensor>, MlxError> {
        let weight = self.dense_as(&format!("{name}.weight"), dtype)?;
        let bias = self.dense_as(&format!("{name}.bias"), dtype)?;
        Conv2d::new(weight, Some(bias)).map_err(|err| MlxError::new(err.to_string()))
    }

    /// 线性层，权重按 `bits` 位分组反量化到 `dtype`。
    ///
    /// 分组权重的键名带 `.scales` 与 `.biases`；`.bias` 是另一回事——那是线性层
    /// 自己的偏置，可能没有。
    pub(crate) fn affine_as(
        &mut self,
        name: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<Linear<MlxTensor>, MlxError> {
        let weight = self.dequant_as(
            &format!("{name}.weight"),
            &format!("{name}.scales"),
            &format!("{name}.biases"),
            group_size,
            bits,
            dtype,
        )?;
        let bias_name = format!("{name}.bias");
        let bias = if self.has(&bias_name) {
            Some(self.dense_as(&bias_name, dtype)?)
        } else {
            None
        };
        let layer = Linear::new(weight, bias).map_err(|err| MlxError::new(err.to_string()))?;
        layer.weight().array.eval().map_err(from_mlx)?;
        if let Some(bias) = layer.bias() {
            bias.array.eval().map_err(from_mlx)?;
        }
        Ok(layer)
    }

    /// 线性层，权重保持分组形状（打包值 + 缩放 + 偏置），不预先反量化。
    ///
    /// 这样前向就能直接走量化矩阵乘：整层的 f16 稠密表从不产生，文件里有多少
    /// 打包字节就读多少。反量化的口子见 `affine_as`，只给不经量化矩阵乘的
    /// 路径（例如激活不是 f16）与归因对照用。
    pub(crate) fn affine_packed(
        &mut self,
        name: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<Linear<MlxTensor>, MlxError> {
        let weight = self.grouped_tensor(
            &format!("{name}.weight"),
            group_size,
            bits,
            dtype,
        )?;
        let bias_name = format!("{name}.bias");
        let bias = if self.has(&bias_name) {
            Some(self.dense_as(&bias_name, dtype)?)
        } else {
            None
        };
        Linear::new(weight, bias).map_err(|err| MlxError::new(err.to_string()))
    }

    /// 整张分组权重（例如词嵌入表）原样常驻：三块都留在设备上，一元素不还原。
    ///
    /// 词嵌入与 `lm_head` 是同一张 151936×1024 的表（`tie_word_embeddings`）。
    /// 以前的做法是先把整张表反量化成 f16（311 MB）再按行取；现在常驻的就是
    /// 文件里那三块（约 78 + 5 + 5 MB），取几十行时才还原那几十行。
    pub(crate) fn affine_table_packed(
        &mut self,
        name: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<MlxTensor, MlxError> {
        self.grouped_tensor(&format!("{name}.weight"), group_size, bits, dtype)
    }

    /// 把 `key` 这一张分组权重的三块读上设备，逻辑形状按 `(输出, 输入)` 记。
    ///
    /// 打包宽度与组大小必须对得上：一行 `输入` 个数在 {bits} 位下占
    /// `输入 * bits / 32` 个 u32，缩放与偏置各占 `输入 / 组大小` 个。
    fn grouped_tensor(
        &mut self,
        key: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<MlxTensor, MlxError> {
        if group_size == 0 || !voice_tensor::GROUP_SIZE.is_multiple_of(group_size) {
            return Err(MlxError::new(format!("组大小 {group_size} 不是 64 的因子")));
        }
        if !matches!(bits, 2 | 3 | 4 | 5 | 6 | 8) {
            return Err(MlxError::new(format!("分组位数 {bits} 不受支持")));
        }
        let stem = key
            .strip_suffix(".weight")
            .ok_or_else(|| MlxError::new(format!("{key} 不是权重键")))?;
        let packed_loc = self.locate(key)?;
        let scale_loc = self.locate(&format!("{stem}.scales"))?;
        let bias_loc = self.locate(&format!("{stem}.biases"))?;
        if packed_loc.dtype != "U32" {
            return Err(MlxError::new(format!("{key} 不是分组权重")));
        }
        if packed_loc.shape.len() != 2 || scale_loc.shape.len() != 2 || bias_loc.shape.len() != 2 {
            return Err(MlxError::new(format!("{key} 不是二维分组权重")));
        }
        let rows = packed_loc.shape[0];
        if rows == 0 {
            return Err(MlxError::new(format!("{key} 有 0 行")));
        }
        let width = packed_loc.shape[1]
            .checked_mul(32 / bits)
            .ok_or_else(|| MlxError::new(format!("{key} 的输入维溢出")))?;
        if !width.is_multiple_of(group_size) {
            return Err(MlxError::new(format!(
                "{key} 的输入维 {width} 不能按组大小 {group_size} 划分"
            )));
        }
        let groups = width / group_size;
        if scale_loc.shape != [rows, groups] || bias_loc.shape != [rows, groups] {
            return Err(MlxError::new(format!("{key} 的缩放或偏置形状不对")));
        }

        let packed_bytes = self.read_range(&packed_loc)?;
        let scale_bytes = self.read_range(&scale_loc)?;
        let bias_bytes = self.read_range(&bias_loc)?;
        let packed = u32_le(&packed_bytes)?;
        drop(packed_bytes);
        let scale_f = floats_from(&scale_bytes, &scale_loc, &format!("{stem}.scales"))?;
        let bias_f = floats_from(&bias_bytes, &bias_loc, &format!("{stem}.biases"))?;
        if packed.len() != rows * packed_loc.shape[1]
            || scale_f.len() != rows * groups
            || bias_f.len() != rows * groups
        {
            return Err(MlxError::new(format!("{key} 的长度不对")));
        }

        let target = mlx_dtype(dtype)?;
        let packed_array = Array::from_slice(&packed, &i32_dims(&packed_loc.shape)?);
        let scale_array = Array::from_slice(&scale_f, &i32_dims(&scale_loc.shape)?)
            .as_dtype(target)
            .map_err(from_mlx)?;
        let bias_array = Array::from_slice(&bias_f, &i32_dims(&bias_loc.shape)?)
            .as_dtype(target)
            .map_err(from_mlx)?;
        // 三块都求值，求完就能把主机上的字节放开；不拼稠密表。
        packed_array.eval().map_err(from_mlx)?;
        scale_array.eval().map_err(from_mlx)?;
        bias_array.eval().map_err(from_mlx)?;
        Ok(MlxTensor::grouped(
            packed_array,
            scale_array,
            Some(bias_array),
            &[rows, width],
            dtype,
            group_size,
            bits,
        ))
    }

    /// 层归一化：`<name>.weight` 与 `<name>.bias`，eps 由调用方给。
    pub(crate) fn layer_norm(
        &mut self,
        name: &str,
        eps: f32,
        dtype: DType,
    ) -> Result<LayerNorm<MlxTensor>, MlxError> {
        let weight = self.dense_as(&format!("{name}.weight"), dtype)?;
        let bias = self.dense_as(&format!("{name}.bias"), dtype)?;
        LayerNorm::new(weight, Some(bias), eps).map_err(|err| MlxError::new(err.to_string()))
    }

    /// 分组权重反量化到 `dtype`。
    fn dequant_as(
        &mut self,
        weight: &str,
        scales: &str,
        biases: &str,
        group_size: usize,
        bits: usize,
        dtype: DType,
    ) -> Result<MlxTensor, MlxError> {
        if group_size == 0 || !voice_tensor::GROUP_SIZE.is_multiple_of(group_size) {
            return Err(MlxError::new(format!("组大小 {group_size} 不是 64 的因子")));
        }
        let packed_loc = self.locate(weight)?;
        let scale_loc = self.locate(scales)?;
        let bias_loc = self.locate(biases)?;
        if packed_loc.dtype != "U32" {
            return Err(MlxError::new(format!("{weight} 不是分组权重")));
        }
        let packed_bytes = self.read_range(&packed_loc)?;
        let scale_bytes = self.read_range(&scale_loc)?;
        let bias_bytes = self.read_range(&bias_loc)?;
        let packed = u32_le(&packed_bytes)?;
        let scale_f = match scale_loc.dtype.as_str() {
            "F16" => f16_le(&scale_bytes)?,
            "F32" => f32_le(&scale_bytes)?,
            other => return Err(MlxError::new(format!("{scales} 的数据类型 {other}"))),
        };
        let bias_f = match bias_loc.dtype.as_str() {
            "F16" => f16_le(&bias_bytes)?,
            "F32" => f32_le(&bias_bytes)?,
            other => return Err(MlxError::new(format!("{biases} 的数据类型 {other}"))),
        };
        if packed.len() != numel(&packed_loc.shape)? || scale_f.len() != numel(&scale_loc.shape)? {
            return Err(MlxError::new(format!("{weight} 的打包长度不对")));
        }
        if bias_f.len() != numel(&bias_loc.shape)? {
            return Err(MlxError::new(format!("{biases} 的长度不对")));
        }
        self.dequantize_block(
            &packed,
            &packed_loc.shape,
            &scale_f,
            &scale_loc.shape,
            &bias_f,
            &bias_loc.shape,
            group_size,
            bits,
            dtype,
            weight,
        )
    }

    /// 把打包权重、缩放、偏置三块主机数据交给 `mlx` 反量化，再转成 `dtype`。
    ///
    /// `dequant_as` 与 `affine_table` 共用这一段：前者给一整张，后者给分块。
    #[allow(clippy::too_many_arguments)]
    fn dequantize_block(
        &self,
        packed: &[u32],
        packed_dims: &[usize],
        scale_f: &[f32],
        scale_dims: &[usize],
        bias_f: &[f32],
        bias_dims: &[usize],
        group_size: usize,
        bits: usize,
        dtype: DType,
        name: &str,
    ) -> Result<MlxTensor, MlxError> {
        let packed_array = Array::from_slice(packed, &i32_dims(packed_dims)?);
        let scale_array = Array::from_slice(scale_f, &i32_dims(scale_dims)?);
        let bias_array = Array::from_slice(bias_f, &i32_dims(bias_dims)?);
        let bits_i32 =
            i32::try_from(bits).map_err(|_| MlxError::new(format!("分组位数 {bits} 超出 i32")))?;
        let restored = ops::dequantize(
            &packed_array,
            &scale_array,
            Some(&bias_array),
            DequantizeOptions {
                mode: QuantizationMode::Affine,
                group_size: Some(i32::try_from(group_size).unwrap_or(64)),
                bits: Some(bits_i32),
            },
        )
        .map_err(from_mlx)?;
        let target = mlx_dtype(dtype)?;
        let restored = restored.as_dtype(target).map_err(from_mlx)?;
        restored.eval().map_err(from_mlx)?;
        let dims: Vec<usize> = restored
            .shape()
            .iter()
            .map(|dim| usize::try_from(*dim).unwrap_or(0))
            .collect();
        if dims.iter().any(|dim| *dim == 0) {
            return Err(MlxError::new(format!("{name} 反量化形状无效")));
        }
        Ok(MlxTensor::dense(restored, dtype))
    }

    fn locate(&self, key: &str) -> Result<TensorLoc, MlxError> {
        let object = object_for(&self.header, key)?;
        let dtype = json_string(object, "dtype")?;
        let shape = json_ints(object, "shape")?;
        let offsets = json_ints(object, "data_offsets")?;
        if offsets.len() != 2 || offsets[1] < offsets[0] {
            return Err(MlxError::new(format!("{key} 的数据区间无效")));
        }
        Ok(TensorLoc {
            dtype,
            shape,
            start: offsets[0] as u64,
            end: offsets[1] as u64,
        })
    }

    fn read_range(&mut self, loc: &TensorLoc) -> Result<Vec<u8>, MlxError> {
        let len = loc
            .end
            .checked_sub(loc.start)
            .ok_or_else(|| MlxError::new("数据区间回绕"))?;
        if len > 1024 * 1024 * 1024 {
            return Err(MlxError::new("单个权重超过 1GB"));
        }
        self.file
            .seek(SeekFrom::Start(self.data_base + loc.start))
            .map_err(|_| MlxError::new("读权重失败"))?;
        let mut buf = vec![0u8; len as usize];
        self.file
            .read_exact(&mut buf)
            .map_err(|_| MlxError::new("读权重失败"))?;
        Ok(buf)
    }
}

fn mlx_dtype(dtype: DType) -> Result<Dtype, MlxError> {
    match dtype {
        DType::F32 => Ok(Dtype::Float32),
        DType::F16 => Ok(Dtype::Float16),
        DType::I32 => Ok(Dtype::Int32),
        DType::I16 => Ok(Dtype::Int16),
        DType::Bool => Ok(Dtype::Bool),
    }
}

pub(crate) fn dense_of(data: &[f32], dims: &[usize], dtype: DType) -> Result<MlxTensor, MlxError> {
    let shape = i32_dims(dims)?;
    let product = numel(dims)?;
    if product != data.len() {
        return Err(MlxError::new("张量元素个数与形状不一致"));
    }
    let array = Array::from_slice(data, &shape);
    let array = if dtype == DType::F32 {
        array
    } else {
        array.as_dtype(mlx_dtype(dtype)?).map_err(from_mlx)?
    };
    Ok(MlxTensor::dense(array, dtype))
}

fn object_for<'a>(header: &'a str, key: &str) -> Result<&'a str, MlxError> {
    let pattern = format!("\"{key}\"");
    let at = header
        .find(&pattern)
        .ok_or_else(|| MlxError::new(format!("权重里没有 {key}")))?;
    let after = &header[at + pattern.len()..];
    let brace = after
        .find('{')
        .ok_or_else(|| MlxError::new(format!("{key} 没有对象")))?;
    let bytes = after[brace..].as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match *byte {
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&after[brace..brace + index + 1]);
                }
            }
            _ => {}
        }
    }
    Err(MlxError::new(format!("{key} 的对象没有结束")))
}

fn json_string(object: &str, field: &str) -> Result<String, MlxError> {
    let pattern = format!("\"{field}\"");
    let at = object
        .find(&pattern)
        .ok_or_else(|| MlxError::new(format!("缺少 {field}")))?;
    let rest = &object[at + pattern.len()..];
    let quote = rest
        .find('"')
        .ok_or_else(|| MlxError::new(format!("{field} 不是字符串")))?;
    let tail = &rest[quote + 1..];
    let end = tail
        .find('"')
        .ok_or_else(|| MlxError::new(format!("{field} 没有结束引号")))?;
    Ok(tail[..end].to_string())
}

fn json_ints(object: &str, field: &str) -> Result<Vec<usize>, MlxError> {
    let pattern = format!("\"{field}\"");
    let at = object
        .find(&pattern)
        .ok_or_else(|| MlxError::new(format!("缺少 {field}")))?;
    let rest = &object[at + pattern.len()..];
    let open = rest
        .find('[')
        .ok_or_else(|| MlxError::new(format!("{field} 不是数组")))?;
    let close = rest[open..]
        .find(']')
        .ok_or_else(|| MlxError::new(format!("{field} 没有结束括号")))?;
    let body = &rest[open + 1..open + close];
    if body.trim().is_empty() {
        return Ok(Vec::new());
    }
    body.split(',')
        .map(|part| {
            part.trim()
                .parse::<usize>()
                .map_err(|_| MlxError::new(format!("{field} 含有非整数字")))
        })
        .collect()
}

fn numel(shape: &[usize]) -> Result<usize, MlxError> {
    shape
        .iter()
        .try_fold(1usize, |acc, dim| acc.checked_mul(*dim))
        .ok_or_else(|| MlxError::new("形状溢出"))
}

fn i32_dims(dims: &[usize]) -> Result<Vec<i32>, MlxError> {
    dims.iter()
        .map(|dim| i32::try_from(*dim).map_err(|_| MlxError::new(format!("维度 {dim} 超出 i32"))))
        .collect()
}

fn f32_le(bytes: &[u8]) -> Result<Vec<f32>, MlxError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(MlxError::new("F32 字节数不是 4 的倍数"));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn u32_le(bytes: &[u8]) -> Result<Vec<u32>, MlxError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(MlxError::new("U32 字节数不是 4 的倍数"));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

/// 按表头记下的 dtype 把字节读成 f32。缩量与偏置可能是 f16 也可能是 f32。
fn floats_from(bytes: &[u8], loc: &TensorLoc, key: &str) -> Result<Vec<f32>, MlxError> {
    match loc.dtype.as_str() {
        "F16" => f16_le(bytes),
        "F32" => f32_le(bytes),
        other => Err(MlxError::new(format!("{key} 的数据类型 {other} 不是浮点"))),
    }
}

fn f16_le(bytes: &[u8]) -> Result<Vec<f32>, MlxError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(MlxError::new("F16 字节数不是 2 的倍数"));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|chunk| f16_to_f32(u16::from_le_bytes([chunk[0], chunk[1]])))
        .collect())
}

/// f16 位型转 f32。只用于把权重读进主机，不参与前向。
pub(crate) fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let frac = (bits & 0x3ff) as u32;
    let out_bits = if exp == 0 {
        if frac == 0 {
            sign << 31
        } else {
            let mut mant = frac;
            let mut exponent = 127 - 15 + 1;
            while mant & 0x400 == 0 {
                mant <<= 1;
                exponent -= 1;
            }
            mant &= 0x3ff;
            (sign << 31) | (exponent << 23) | (mant << 13)
        }
    } else if exp == 31 {
        (sign << 31) | (0xff << 23) | (frac << 13)
    } else {
        let exponent = exp + (127 - 15);
        (sign << 31) | (exponent << 23) | (frac << 13)
    };
    f32::from_bits(out_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f16_bit_patterns_round_trip_through_the_host_path() {
        // 1.0、-2.0、最大有限值、最小非零次正规数。
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x7bff), 65504.0);
        assert_eq!(f16_to_f32(0x0001), 5.9604645e-8);
        assert_eq!(f16_to_f32(0x0000), 0.0);
    }

    #[test]
    fn json_helpers_read_a_tensor_entry() {
        let object = r#"{"dtype":"F16","shape":[480,3,3,1],"data_offsets":[0,2880]}"#;
        assert_eq!(json_string(object, "dtype").unwrap(), "F16");
        assert_eq!(json_ints(object, "shape").unwrap(), vec![480, 3, 3, 1]);
        assert_eq!(json_ints(object, "data_offsets").unwrap(), vec![0, 2880]);
    }
}
