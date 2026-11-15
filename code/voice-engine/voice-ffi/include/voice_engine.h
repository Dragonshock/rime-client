#ifndef VOICE_ENGINE_H
#define VOICE_ENGINE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 识别整段的回调。is_final 为 0 是部分结果，为 1 是定稿。
   utf8 只在这次回调返回前有效，调用方要自己复制。
   回调发生在引擎线程，不在音频线程。
   装上 reply_sink 之后本回调不再被调用，两个口不会重复发同一条。 */
typedef void (*voice_text_fn)(uint64_t session, const char *utf8, int is_final, void *ctx);

/* 一行 JSON 的回复。键就是 RecognitionReplyParser 读的六个：
   session(number)、text(string)、decoded(bool)、kind("final"|"partial")、
   ok(bool)、message(string)。末尾不带换行，只在这条回调返回前有效。
   定稿失败时这里发一行 ok=false，message 是原因，文本为空。
   kind 还有第三种 "replaced"：同号的旧会话被替换掉，文本为空，只进日志。 */
typedef void (*voice_reply_fn)(const char *json_line, void *ctx);

/* 模型目录打不开时返回空指针。Swift 见到空指针就不创建会话。
   编译打开 metal 时，metallib 必须是已存在的文件，引擎线程把它交给 voice-mlx。
   关闭 metal 时不加载着色器，会话仍用假后端。 */
void *voice_engine_new(const char *model_dir, const char *metallib);
void voice_engine_free(void *engine);

/* 返回 0 表示引擎已接受。非 0 表示未接受。
   begin 撞上引擎里还挂着的同号会话时不拒绝：先丢掉那枚旧会话（解码器与状态一起
   释放），再按这次的热词建新的。替换会从 reply_sink 发一行 kind="replaced"。 */
int voice_session_begin(void *engine, uint64_t session, const char *hotwords_or_null);
int voice_session_feed(void *engine, uint64_t session, const int16_t *pcm, size_t n);
int voice_session_finish(void *engine, uint64_t session);

/* 丢掉引擎侧的一枚会话，释放它的解码器与状态，号随即可以再用。
   取消（失焦、看门狗）不发 finish，用这一句收场。没有那枚会话也返回 0；
   引擎指针为空返回 -1。 */
int voice_session_discard(void *engine, uint64_t session);

void voice_engine_set_callback(void *engine, voice_text_fn fn, void *ctx);

/* 设置一行 JSON 的回复口。装上之后文本事件只走这里，不再调 voice_text_fn。 */
void voice_engine_set_reply_sink(void *engine, voice_reply_fn fn, void *ctx);

/* 上一次 voice_session_finish 失败的原因。返回写入的字节数（不含结尾 NUL）。
   没有失败返回 0；指针为空或缓冲区不够返回 -1。 */
int voice_engine_last_error(void *engine, char *utf8, size_t n);

#ifdef __cplusplus
}
#endif

#endif /* VOICE_ENGINE_H */
