# Prompt List

## 2026

### 8/5

- 打开软件后有个黑框，请去除
- 翻译语言除了列表中的语言，还应允许用户自定义
- 翻译word文档时，会有以下错误：missing field `text` at line 1 column 150
- drop file 仅在网页版有效，desptop版无效
- source / target language加一个switch按钮
- 加一个历史记录功能
- 打开后无任何操作也会占用很高的CPU，分别是：
    * webView2管理器
    * webView2：tranova
    * webView2实用工具：network service

### 8/6

- 文件翻译后点击下载没有反应
- 翻译后的文件支持两种形式：
    1. 仅包含译文
    2. 原文译文混合
- 文件翻译时每次只翻译一段，效率太低，一次翻译多段（具体多少在设置里设置token数（或近似字符数）上限），提高效率
- 调用AI可以并发，并发数也在设置里设置
- 文件翻译显示进度条详情
- 翻译文件的同时允许进入其它tab，回来进度不会丢失；
期间可以进行其它文本翻译，且文本翻译优先级更高（注意处理好AI调用的并发）
- 修复bug：source和target language在离开当前tab再进来时就丢失了


### 8/7
- 文件翻译失败了，ollama和其openai兼容接口失败信息如下
Network request failed: error sending request for url (http://127.0.0.1:11434/api/chat)
Network request failed: error sending request for url (http://127.0.0.1:11434/v1/chat/completions)
但是我在ollama日志已经看到请求了，说明网络本身没问题

- version:bump 加一个显示当前版本号的功能
- 15分钟超时有点太过了，这个时间让用户设置吧，默认180秒
- 你在测试时可以使用本地ollama（不要使用ollama命令，而是curl或者直接rust代码）
