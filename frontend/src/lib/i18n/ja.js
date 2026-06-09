// MACHINE-DRAFT — pending native-speaker review. Falls back to en via spread for any key not yet reviewed.
//
// Japanese — high-traffic keys carry best-effort machine translations; the
// long tail spreads from the English source via `...en` so any unreviewed
// key still renders. See en.js for the flat dotted-key layout rationale.
import en from './en.js';

const ja = {
  ...en,

  // ── dashboard ────────────────────────────────────────────────────
  'dashboard.title':                 'ダッシュボード',
  'dashboard.search_placeholder':    'モニターを検索…',
  'dashboard.add_monitor':           'モニターを追加',
  'dashboard.status_page':           'ステータスページ',
  'dashboard.sign_out':              'サインアウト',
  'dashboard.nav.title':             'メニュー',
  'dashboard.response_time':         '応答時間',
  'dashboard.recent_incidents':      '最近のインシデント',
  'dashboard.view_all':              'すべて表示',
  'dashboard.maintenance':           'メンテナンス',
  'dashboard.all_monitors':          'すべてのモニター',
  'dashboard.uptime_24h':            '24時間の稼働率',
  'dashboard.kpi.up':                '稼働中',
  'dashboard.kpi.warn':              '警告',
  'dashboard.kpi.down':              '停止',
  'dashboard.kpi.paused':            '一時停止',
  'dashboard.loading':               '読み込み中…',
  'dashboard.loading_monitors':      'モニターを読み込み中…',

  // ── common ───────────────────────────────────────────────────────
  'common.cancel':                   'キャンセル',
  'common.save':                     '保存',
  'common.delete':                   '削除',
  'common.clear':                    'クリア',
  'common.edit':                     '編集',
  'common.close':                    '閉じる',
  'common.test':                     'テスト',
  'common.clone':                    '複製',
  'common.pause':                    '一時停止',
  'common.resume':                   '再開',
  'common.loading':                  '読み込み中…',
  'common.saving':                   '保存中…',
  'common.dashboard':                'ダッシュボード',
  'common.name':                     '名前',
  'common.optional':                 '任意',

  // ── notifications (high-traffic nav + buttons) ───────────────────
  'notifications.title':             '通知',
  'notifications.add_channel':       'チャンネルを追加',
  'notifications.close_form':        'フォームを閉じる',
  'notifications.tab.channels':      'チャンネル',
  'notifications.tab.templates':     'テンプレート',
  'notifications.channel.enabled':   '有効',
  'notifications.channel.disabled':  '無効',
  'notifications.enable_push':       'プッシュを有効化',
  'notifications.subscribed':        '登録済み',

  // ── maintenance (high-traffic nav + buttons) ─────────────────────
  'maintenance.title':               'メンテナンス期間',
  'maintenance.new_window':          '新しい期間',
  'maintenance.form.create_window':  '期間を作成',
  'maintenance.form.save_changes':   '変更を保存',

  // ── monitor detail (tabs + action buttons) ───────────────────────
  'monitor.action.test_now':         '今すぐテスト',
  'monitor.action.csv':              'CSV',
  'monitor.tab.overview':            '概要',
  'monitor.tab.heartbeats':          'ハートビート',
  'monitor.tab.config':              '設定',
  'monitor.filter.all':              'すべて',
  'monitor.filter.failures':         '失敗',

  // ── locale picker ────────────────────────────────────────────────
  'locale.picker.label':             '言語',
};

export default ja;
