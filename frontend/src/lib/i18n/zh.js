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

  // ── locale picker ────────────────────────────────────────────────
  'locale.picker.label':             '语言',
};

export default zh;
