export const locales = {
  en: {
    settings: 'Settings',
    servers: 'Servers',
    auth: 'Authentication',
    proxies: 'Proxies',
    general: 'General',
    appearance: 'Appearance',
    theme: 'Theme',
    language: 'Language',
    system: 'System',
    light: 'Light',
    dark: 'Dark',
    terminal: 'Terminal',
    fontFamily: 'Font Family',
    fontSize: 'Font Size',
    cursorStyle: 'Cursor Style',
    scrollback: 'Scrollback Lines',
    webdav: 'WebDAV Sync',
    webdavUrl: 'WebDAV URL',
    username: 'Username',
    password: 'Password',
    confirmations: 'Confirmations',
    confirmCloseTab: 'Confirm before closing tabs',
    confirmExitApp: 'Confirm before exiting application',
    saveStatus: {
      saving: 'Saving...',
      saved: 'Saved',
      error: 'Error saving'
    }
  },
  'zh-CN': {
    settings: '设置',
    servers: '服务器',
    auth: '身份验证',
    proxies: '代理设置',
    general: '常规',
    appearance: '外观',
    theme: '主题',
    language: '语言',
    system: '跟随系统',
    light: '浅色',
    dark: '深色',
    terminal: '终端',
    fontFamily: '字体名称',
    fontSize: '字体大小',
    cursorStyle: '光标样式',
    scrollback: '滚动回溯行数',
    webdav: 'WebDAV 同步',
    webdavUrl: 'WebDAV 地址',
    username: '用户名',
    password: '密码',
    confirmations: '确认操作',
    confirmCloseTab: '关闭标签页前确认',
    confirmExitApp: '退出应用前确认',
    saveStatus: {
      saving: '正在保存...',
      saved: '已保存',
      error: '保存出错'
    }
  }
};

export type LocaleType = keyof typeof locales;
export type TranslationKeys = typeof locales.en;
