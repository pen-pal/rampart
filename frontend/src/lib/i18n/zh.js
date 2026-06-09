// MACHINE-DRAFT — pending native-speaker review. Falls back to en via spread for any key not yet reviewed.
//
// Chinese (Simplified) — high-traffic keys carry best-effort machine
// translations; the long tail spreads from the English source via `...en`
// so any unreviewed key still renders. See en.js for the flat dotted-key
// layout rationale.
import en from './en.js';

const zh = {
  ...en,

  // ── dashboard ────────────────────────────────────────────────────
  'dashboard.title':                 '仪表板',
  'dashboard.search_placeholder':    '搜索监控项…',
  'dashboard.add_monitor':           '添加监控项',
  'dashboard.status_page':           '状态页',
  'dashboard.sign_out':              '退出登录',
  'dashboard.nav.title':             '菜单',
  'dashboard.response_time':         '响应时间',
  'dashboard.recent_incidents':      '近期事件',
  'dashboard.view_all':              '查看全部',
  'dashboard.maintenance':           '维护',
  'dashboard.all_monitors':          '所有监控项',
  'dashboard.uptime_24h':            '24 小时可用率',
  'dashboard.kpi.up':                '正常',
  'dashboard.kpi.warn':              '警告',
  'dashboard.kpi.down':              '宕机',
  'dashboard.kpi.paused':            '已暂停',
  'dashboard.loading':               '加载中…',
  'dashboard.loading_monitors':      '正在加载监控项…',

  // ── common ───────────────────────────────────────────────────────
  'common.cancel':                   '取消',
  'common.save':                     '保存',
  'common.delete':                   '删除',
  'common.clear':                    '清除',
  'common.edit':                     '编辑',
  'common.close':                    '关闭',
  'common.test':                     '测试',
  'common.clone':                    '克隆',
  'common.pause':                    '暂停',
  'common.resume':                   '恢复',
  'common.loading':                  '加载中…',
  'common.saving':                   '保存中…',
  'common.dashboard':                '仪表板',
  'common.name':                     '名称',
  'common.optional':                 '可选',

  // ── notifications (high-traffic nav + buttons) ───────────────────
  'notifications.title':             '通知',
  'notifications.add_channel':       '添加渠道',
  'notifications.close_form':        '关闭表单',
  'notifications.tab.channels':      '渠道',
  'notifications.tab.templates':     '模板',
  'notifications.channel.enabled':   '已启用',
  'notifications.channel.disabled':  '已禁用',
  'notifications.enable_push':       '启用推送',
  'notifications.subscribed':        '已订阅',

  // ── maintenance (high-traffic nav + buttons) ─────────────────────
  'maintenance.title':               '维护时段',
  'maintenance.new_window':          '新建时段',
  'maintenance.form.create_window':  '创建时段',
  'maintenance.form.save_changes':   '保存更改',

  // ── monitor detail (tabs + action buttons) ───────────────────────
  'monitor.action.test_now':         '立即测试',
  'monitor.action.csv':              'CSV',
  'monitor.tab.overview':            '概览',
  'monitor.tab.heartbeats':          '心跳',
  'monitor.tab.config':              '配置',
  'monitor.filter.all':              '全部',
  'monitor.filter.failures':         '失败',

  // ── login (high-traffic) ─────────────────────────────────────────
  'login.subtitle_signin':           '登录',
  'login.subtitle_setup':            '创建你的管理员账户',
  'login.subtitle_totp':             '两步验证',
  'login.email':                     '邮箱',
  'login.name':                      '名称',
  'login.password':                  '密码',
  'login.sign_in':                   '登录',
  'login.create_admin':              '创建管理员账户',
  'login.verify':                    '验证',
  'login.totp_code':                 '验证码',

  // ── status pages (high-traffic) ──────────────────────────────────
  'statuspage.title':                '状态页',
  'statuspage.new':                  '新建状态页',
  'statuspage.empty.title':          '还没有状态页',
  'statuspage.back':                 '返回',
  'statuspage.field_title':          '标题',
  'statuspage.subscribers':          '订阅者',
  'statuspage.incident.title':       '事件',
  'statuspage.incident.post':        '发布',
  'statuspage.incident.resolve':     '解决',

  // ── new monitor wizard (high-traffic) ────────────────────────────
  'wizard.new_monitor':              '新建监控',
  'wizard.step.type':                '类型',
  'wizard.step.configure':           '配置',
  'wizard.step.schedule':            '计划',
  'wizard.back':                     '返回',
  'wizard.continue':                 '继续',
  'wizard.create_monitor':           '创建监控',
  'wizard.creating':                 '创建中…',
  'wizard.example':                  '示例',
  'wizard.live_preview':             '实时预览',
  'wizard.step1.title':              '你想监控什么？',
  'wizard.step2.title':              '告诉我们要检查什么。',
  'wizard.field.display_name':       '显示名称',
  'wizard.field.url':                'URL',
  'wizard.field.hostname':           '主机名',
  'wizard.field.port':               '端口',
  'wizard.field.notifications':      '通知',

  // ── api keys (high-traffic) ──────────────────────────────────────
  'apikeys.title':                   'API 密钥',
  'apikeys.new':                     '新建密钥',
  'apikeys.empty.title':             '还没有 API 密钥',
  'apikeys.revoke':                  '撤销',
  'apikeys.name':                    '名称',
  'apikeys.generate':                '生成密钥',
  'apikeys.copy':                    '复制',
  'apikeys.copied':                  '已复制',

  // ── proxies (high-traffic) ───────────────────────────────────────
  'proxies.title':                   '代理',
  'proxies.new':                     '新建代理',
  'proxies.empty.title':             '还没有代理',
  'proxies.state.active':            '活动',
  'proxies.state.paused':            '已暂停',
  'proxies.host':                    '主机',
  'proxies.port':                    '端口',
  'proxies.create':                  '创建代理',

  // ── users (high-traffic) ─────────────────────────────────────────
  'users.title':                     '用户',
  'users.new':                       '新建用户',
  'users.role.admin':                '管理员',
  'users.role.editor':               '编辑者',
  'users.role.readonly':             '只读',
  'users.role_label':                '角色',
  'users.you':                       '你',
  'users.email':                     '邮箱',
  'users.create':                    '创建用户',

  // ── folders (high-traffic) ───────────────────────────────────────
  'folders.title':                   '文件夹',
  'folders.create':                  '创建文件夹',
  'folders.empty':                   '还没有文件夹。在上方创建一个。',
  'folders.rename':                  '重命名',
  'folders.monitors':                '监控',
  'folders.tags':                    '标签',
  'folders.root':                    '根',

  // ── tags (high-traffic) ──────────────────────────────────────────
  'tags.title':                      '标签',
  'tags.create':                     '创建标签',
  'tags.empty':                      '还没有标签。在上方创建一个。',

  // ── settings (high-traffic) ──────────────────────────────────────
  'settings.smtp.title':             'SMTP 设置',
  'settings.smtp.host':              '主机',
  'settings.smtp.port':              '端口',
  'settings.smtp.username':          '用户名',
  'settings.smtp.password':          '密码',
  'settings.retention.title':        '保留期',

  // ── security (high-traffic) ──────────────────────────────────────
  'security.title':                  '安全',
  'security.pw.title':               '修改密码',
  'security.pw.current':             '当前',
  'security.pw.new':                 '新',
  'security.pw.update':              '更新密码',
  'security.totp.title':             '两步验证',
  'security.totp.on':                '开启',
  'security.totp.activate':          '激活',
  'security.totp.turn_off':          '关闭两步验证',

  // ── audit log (high-traffic) ─────────────────────────────────────
  'audit.title':                     '审计日志',
  'audit.all_kinds':                 '所有类型',
  'audit.all_actors':                '所有操作者',
  'audit.col.when':                  '时间',
  'audit.col.actor':                 '操作者',
  'audit.col.action':                '操作',
  'audit.col.resource':              '资源',
  'audit.col.payload':               '载荷',
  'audit.empty':                     '还没有审计条目。',
  'audit.load_more':                 '加载更多',
  'audit.actor_system':              '系统',

  // ── locale picker ────────────────────────────────────────────────
  'locale.picker.label':             '语言',
};

export default zh;
