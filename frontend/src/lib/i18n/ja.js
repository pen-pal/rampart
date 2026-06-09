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

  // ── login (high-traffic) ─────────────────────────────────────────
  'login.subtitle_signin':           'サインイン',
  'login.subtitle_setup':            '管理者アカウントを作成',
  'login.subtitle_totp':             '二要素認証',
  'login.email':                     'メール',
  'login.name':                      '名前',
  'login.password':                  'パスワード',
  'login.sign_in':                   'サインイン',
  'login.create_admin':              '管理者アカウントを作成',
  'login.verify':                    '確認',
  'login.totp_code':                 'コード',

  // ── status pages (high-traffic) ──────────────────────────────────
  'statuspage.title':                'ステータスページ',
  'statuspage.new':                  '新しいステータスページ',
  'statuspage.empty.title':          'ステータスページはまだありません',
  'statuspage.back':                 '戻る',
  'statuspage.field_title':          'タイトル',
  'statuspage.subscribers':          '購読者',
  'statuspage.incident.title':       'インシデント',
  'statuspage.incident.post':        '投稿',
  'statuspage.incident.resolve':     '解決',

  // ── new monitor wizard (high-traffic) ────────────────────────────
  'wizard.new_monitor':              '新しいモニター',
  'wizard.step.type':                'タイプ',
  'wizard.step.configure':           '設定',
  'wizard.step.schedule':            'スケジュール',
  'wizard.back':                     '戻る',
  'wizard.continue':                 '続行',
  'wizard.create_monitor':           'モニターを作成',
  'wizard.creating':                 '作成中…',
  'wizard.example':                  '例',
  'wizard.live_preview':             'ライブプレビュー',
  'wizard.step1.title':              '何を監視しますか？',
  'wizard.step2.title':              '何をチェックするか教えてください。',
  'wizard.field.display_name':       '表示名',
  'wizard.field.url':                'URL',
  'wizard.field.hostname':           'ホスト名',
  'wizard.field.port':               'ポート',
  'wizard.field.notifications':      '通知',

  // ── api keys (high-traffic) ──────────────────────────────────────
  'apikeys.title':                   'API キー',
  'apikeys.new':                     '新しいキー',
  'apikeys.empty.title':             'API キーはまだありません',
  'apikeys.revoke':                  '取り消す',
  'apikeys.name':                    '名前',
  'apikeys.generate':                'キーを生成',
  'apikeys.copy':                    'コピー',
  'apikeys.copied':                  'コピーしました',

  // ── proxies (high-traffic) ───────────────────────────────────────
  'proxies.title':                   'プロキシ',
  'proxies.new':                     '新しいプロキシ',
  'proxies.empty.title':             'プロキシはまだありません',
  'proxies.state.active':            'アクティブ',
  'proxies.state.paused':            '一時停止',
  'proxies.host':                    'ホスト',
  'proxies.port':                    'ポート',
  'proxies.create':                  'プロキシを作成',

  // ── users (high-traffic) ─────────────────────────────────────────
  'users.title':                     'ユーザー',
  'users.new':                       '新しいユーザー',
  'users.role.admin':                '管理者',
  'users.role.editor':               '編集者',
  'users.role.readonly':             '読み取り専用',
  'users.role_label':                'ロール',
  'users.you':                       'あなた',
  'users.email':                     'メール',
  'users.create':                    'ユーザーを作成',

  // ── folders (high-traffic) ───────────────────────────────────────
  'folders.title':                   'フォルダー',
  'folders.create':                  'フォルダーを作成',
  'folders.empty':                   'フォルダーはまだありません。上で作成してください。',
  'folders.rename':                  '名前を変更',
  'folders.monitors':                'モニター',
  'folders.tags':                    'タグ',
  'folders.root':                    'ルート',

  // ── tags (high-traffic) ──────────────────────────────────────────
  'tags.title':                      'タグ',
  'tags.create':                     'タグを作成',
  'tags.empty':                      'タグはまだありません。上で作成してください。',

  // ── settings (high-traffic) ──────────────────────────────────────
  'settings.smtp.title':             'SMTP 設定',
  'settings.smtp.host':              'ホスト',
  'settings.smtp.port':              'ポート',
  'settings.smtp.username':          'ユーザー名',
  'settings.smtp.password':          'パスワード',
  'settings.retention.title':        '保持期間',

  // ── security (high-traffic) ──────────────────────────────────────
  'security.title':                  'セキュリティ',
  'security.pw.title':               'パスワードを変更',
  'security.pw.current':             '現在',
  'security.pw.new':                 '新規',
  'security.pw.update':              'パスワードを更新',
  'security.totp.title':             '二要素認証',
  'security.totp.on':                'オン',
  'security.totp.activate':          '有効化',
  'security.totp.turn_off':          '二要素認証を無効化',

  // ── audit log (high-traffic) ─────────────────────────────────────
  'audit.title':                     '監査ログ',
  'audit.all_kinds':                 'すべての種類',
  'audit.all_actors':                'すべての実行者',
  'audit.col.when':                  '日時',
  'audit.col.actor':                 '実行者',
  'audit.col.action':                'アクション',
  'audit.col.resource':              'リソース',
  'audit.col.payload':               'ペイロード',
  'audit.empty':                     '監査エントリはまだありません。',
  'audit.load_more':                 'さらに読み込む',
  'audit.actor_system':              'システム',

  // ── locale picker ────────────────────────────────────────────────
  'locale.picker.label':             '言語',
};

export default ja;
