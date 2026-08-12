- 修复：Translate页正在进行的翻译任务在切换到其它页再回来就不见了。
- 修复：Translate页不是实时，改成流式，实时显示翻译结果
- 添加一个日志页面，默认不显示，可在设置里开启，并且可以设置级别，默认级别是info。请在程序的关键地方加上日志。日志页面包含两个部分：
    * 系统日志
    * 与AI的对话内容：包括时间，调用的模型，发给AI的内容，AI返回的内容，其它必要信息等等
- 由于Ollama等AI都支持openai兼容接口，因此AI provider仅支持openai兼容接口（openai responses api）就行。但可以提示用户如何填入ollama等服务的openai兼容接口地址
- 如果可以的话，自动获取模型列表，也允许用户输入模型。如果可以的话自动获取模型的上下文大小，也可以用户自行输入。
- 删除字符上限设置项，默认为上下文大小除以4，这个值是和模型绑定的
- segments上限也从设置中移到ai provider中和模型绑定，默认值为16
- 修改重试逻辑：如果某个batch由于AI返回结果解析的原因失败，将这个batch拆分成两个batch，直至只剩一个segment回退到非batch模式
- 并发设置也从设置中移到ai provider中和模型绑定，默认值为2
- ai provider设置模型时添加一个“文本翻译常用模型”选项，默认disable，enable时文件翻译使用这个模型时并发数-1（除非并发数就是1）保留给文本翻译使用，disable时按照设置的并发数并发
- 当segment总数小于等于一个batch的segments上限时，也应该等分成多个（并发数）batch。或者说是：“segment总数”<=“一个batch的segments上限”x“并发数”时
- 文本翻译和文件翻译页添加一个推理程度的下拉菜单，控制ai模型的思考强度
- 文件翻译页面加一个翻译前总结的checkbox，默认enable。enable时截取翻译文件最多达模型上下文大小除以2的文本数量先给AI做一个summary，这个总结会在后续的翻译中附在提示词中，力求让ai翻译时充分理解上下文，提高专有名词的翻译精度。
- history页面中的长文本应该auto wrap
- 文件翻译时间较长，添加一个取消按钮
- 修复文件翻译顺序问题：一个任务还没好，另一个任务加进来应该等待
- 修复：文件翻译完成时，打开文件和打开文件夹按钮点击会出错：Not allowed to open path
- 修改AI返回格式：当前使用json格式，但此格式会提高解析失败的概率，改成如下格式
    * 选取一个合理的分隔符，单独一行，用于分割segments；AI返回的结果也让其用这个分隔符分隔。
    * 如果某个segment包含这个分隔符，则此segment的翻译回退到简单文本的非batch模式
    * 由于只是简单的分隔，使用流式输出后，就能简单地知道当前batch已经翻译了几个segment，从而更好地显示文件翻译进度
- 文件翻译添加pdf支持
- 翻译完excel后，打开excel文件会提示需要修复，修复完后可以正常打开，修复记录如下
```
<recoveryLog xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<logFileName>error125080_02.xml</logFileName>
<summary>在文件“C:\p\xxx-translated.xlsx”中检测到错误</summary>
<repairedRecords>
<repairedRecord>已修复的记录: /xl/sharedStrings.xml 部分的 字符串属性 (字符串)</repairedRecord>
</repairedRecords>
</recoveryLog>
```
- 分析代码中哪些是在重复造轮子，对于代码量比较大的轮子，请使用成熟的库替代，仅限于成熟的库，不要使用那种根本没人维护的小库。对于只有几十行代码的小轮子请保留
