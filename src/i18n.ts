import i18n from "i18next";
import { initReactI18next } from "react-i18next";

const resources = {
  en: {
    translation: {
      nav: { translate: "Translate", files: "Files", history: "History", glossaries: "Glossaries", prompts: "Prompts", providers: "AI providers", settings: "Settings" },
      common: { save: "Save", cancel: "Cancel", delete: "Delete", edit: "Edit", add: "Add", close: "Close", import: "Import", export: "Export", name: "Name", test: "Test", testing: "Testing...", enabled: "Enabled", disabled: "Off", select: "Select", none: "None", ready: "Ready", error: "Error", retry: "Retry", openMenu: "Open menu", closeMenu: "Close menu", productSubtitle: "AI translation studio", noModel: "No model" },
      translate: { title: "Translate text", subtitle: "Translate with a selected AI, prompt and terminology set.", source: "Source", target: "Translation", sourceLanguage: "Source language", targetLanguage: "Target language", detect: "Auto detect", swapLanguages: "Swap source and target languages", provider: "AI provider", prompt: "Prompt", glossaries: "Glossaries", placeholder: "Enter or paste text", action: "Translate", working: "Translating...", copy: "Copy result", copied: "Copied", characters: "{{count}} chars" },
      files: { title: "Translate files", subtitle: "Document structure and formatting are retained where the format permits.", drop: "Drop a file here or choose a file", supported: "DOCX, PPTX, XLSX, TXT, Markdown, HTML, CSV, JSON, SRT, VTT, PNG, JPG and WebP", choose: "Choose file", action: "Translate file", working: "Translating file...", download: "Download translation", segments: "{{count}} segments translated" },
      glossary: { title: "Glossaries", subtitle: "Apply one or more terminology sets to each translation.", new: "New glossary", empty: "No glossaries yet", entries: "{{count}} terms", sourceTerm: "Source term", targetTerm: "Preferred translation", note: "Note", addTerm: "Add term", sourceLanguage: "Source language", targetLanguage: "Target language" },
      prompt: { title: "Prompts", subtitle: "Reusable instructions control tone, audience and formatting.", new: "New prompt", empty: "No prompts yet", content: "Instructions", variables: "Available variables: {{source_language}}, {{target_language}}, {{glossary}} and {{text}}." },
      provider: { title: "AI providers", subtitle: "Configure cloud services and local OpenAI-compatible runtimes.", new: "Add provider", type: "Provider type", baseUrl: "API base URL", model: "Model", apiKey: "API key", apiKeyHint: "Stored only on this device", images: "Supports image input/output", connected: "Provider responded successfully" },
      settings: { title: "Settings", subtitle: "Network, language and local web access.", language: "Interface language", proxy: "Proxy URL", proxyHint: "HTTP, HTTPS, SOCKS5 or SOCKS5H. Leave empty for a direct connection.", webAccess: "Web access", address: "Listening address", port: "Port", webHint: "Restart Tranova after changing the web address.", chunk: "Maximum characters per translation chunk", restart: "Server address changes take effect after restart." },
      history: { title: "Translation history", subtitle: "Review recent text and file translation jobs.", empty: "No translation history yet", clear: "Clear history", remove: "Remove from history", source: "Source", translation: "Translation", file: "File", text: "Text", segments: "File translation" },
      status: { backendUnavailable: "Tranova service is unavailable. Start the desktop app or server.", noProvider: "Configure and enable an AI provider first.", saved: "Saved", imported: "Imported successfully" },
      languages: { en: "English", zh: "Chinese", ja: "Japanese", ko: "Korean", fr: "French", de: "German", es: "Spanish", ru: "Russian", auto: "Auto detect", customPlaceholder: "Enter a language" }
    },
  },
  "zh-CN": {
    translation: {
      nav: { translate: "文本翻译", files: "文件翻译", history: "历史记录", glossaries: "词汇表", prompts: "提示词", providers: "AI 服务", settings: "设置" },
      common: { save: "保存", cancel: "取消", delete: "删除", edit: "编辑", add: "添加", close: "关闭", import: "导入", export: "导出", name: "名称", test: "测试", testing: "测试中…", enabled: "启用", disabled: "停用", select: "选择", none: "无", ready: "就绪", error: "错误", retry: "重试", openMenu: "打开菜单", closeMenu: "关闭菜单", productSubtitle: "AI 翻译工作台", noModel: "未设置模型" },
      translate: { title: "文本翻译", subtitle: "使用指定的 AI、提示词和术语翻译内容。", source: "原文", target: "译文", sourceLanguage: "源语言", targetLanguage: "目标语言", detect: "自动检测", swapLanguages: "交换源语言和目标语言", provider: "AI 服务", prompt: "提示词", glossaries: "词汇表", placeholder: "输入或粘贴需要翻译的内容", action: "翻译", working: "翻译中…", copy: "复制译文", copied: "已复制", characters: "{{count}} 个字符" },
      files: { title: "文件翻译", subtitle: "在格式允许时保留文档结构与排版。", drop: "将文件拖到此处，或选择文件", supported: "支持 DOCX、PPTX、XLSX、TXT、Markdown、HTML、CSV、JSON、SRT、VTT、PNG、JPG 和 WebP", choose: "选择文件", action: "翻译文件", working: "正在翻译文件…", download: "下载译文", segments: "已翻译 {{count}} 个片段" },
      glossary: { title: "词汇表", subtitle: "每次翻译可同时引用一个或多个术语集。", new: "新建词汇表", empty: "还没有词汇表", entries: "{{count}} 个术语", sourceTerm: "原词", targetTerm: "指定译法", note: "备注", addTerm: "添加术语", sourceLanguage: "源语言", targetLanguage: "目标语言" },
      prompt: { title: "提示词", subtitle: "用可复用指令控制语气、受众和输出格式。", new: "新建提示词", empty: "还没有提示词", content: "指令内容", variables: "可用变量：{{source_language}}、{{target_language}}、{{glossary}} 和 {{text}}。" },
      provider: { title: "AI 服务", subtitle: "配置在线服务和本地 OpenAI 兼容模型。", new: "添加服务", type: "服务类型", baseUrl: "API 地址", model: "模型", apiKey: "API 密钥", apiKeyHint: "仅保存在本机", images: "支持图片输入输出", connected: "服务响应正常" },
      settings: { title: "设置", subtitle: "配置网络、界面语言和本地 Web 访问。", language: "界面语言", proxy: "代理地址", proxyHint: "支持 HTTP、HTTPS、SOCKS5 和 SOCKS5H；留空时直连。", webAccess: "Web 访问", address: "监听地址", port: "端口", webHint: "Web 地址修改后需重启 Tranova。", chunk: "单个翻译片段最大字符数", restart: "服务器地址修改后将在重启时生效。" },
      history: { title: "翻译历史", subtitle: "查看最近的文本和文件翻译记录。", empty: "还没有翻译记录", clear: "清空历史记录", remove: "移除记录", source: "原文", translation: "译文", file: "文件", text: "文本", segments: "文件翻译" },
      status: { backendUnavailable: "Tranova 服务不可用，请启动桌面应用或服务器。", noProvider: "请先配置并启用一个 AI 服务。", saved: "已保存", imported: "导入成功" },
      languages: { en: "英语", zh: "中文", ja: "日语", ko: "韩语", fr: "法语", de: "德语", es: "西班牙语", ru: "俄语", auto: "自动检测", customPlaceholder: "输入语言名称" }
    },
  },
} as const;

i18n.use(initReactI18next).init({
  resources,
  lng: localStorage.getItem("tranova-language") || navigator.language,
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export default i18n;
