// English source-of-truth dictionary.
//
// All strings live as a flat `key -> value` object so lookup is a single
// hash hit at render time. Keys are dot-delimited buckets ("dashboard.…",
// "common.…") only as a naming convention — the lookup itself does NOT
// walk a tree. That keeps t() branchless and gzips nicely.
//
// Interpolation uses `{name}` placeholders so translators can re-order
// arguments without touching code (see lib/i18n.js → t()).
const en = {
  // ── dashboard: header ────────────────────────────────────────────
  'dashboard.title':                 'Dashboard',
  'dashboard.search_placeholder':    'Search monitors…',
  'dashboard.add_monitor':           'Add monitor',
  'dashboard.status_page':           'Status page',
  'dashboard.sign_out':              'Sign out',
  'dashboard.nav.title':             'Menu',

  // ── dashboard: sections ──────────────────────────────────────────
  'dashboard.response_time':         'Response time',
  'dashboard.recent_incidents':      'Recent incidents',
  'dashboard.view_all':              'View all',
  'dashboard.maintenance':           'Maintenance',
  'dashboard.all_monitors':          'All monitors',
  'dashboard.uptime_24h':            '24h uptime',

  // ── dashboard: KPI summary card ──────────────────────────────────
  'dashboard.kpi.eyebrow':           'Status · {window}',
  'dashboard.kpi.total':             '{n} total',
  'dashboard.kpi.up':                'up',
  'dashboard.kpi.warn':              'warn',
  'dashboard.kpi.down':              'down',
  'dashboard.kpi.paused':            'paused',

  // ── dashboard: hero / empty states ───────────────────────────────
  'dashboard.hero.first_monitor':    'Create your first monitor to start receiving heartbeats.',
  'dashboard.empty.title':           'No monitors yet',
  'dashboard.empty.cta':             'Add your first monitor',
  'dashboard.empty.create_first':    'Create the first one',
  'dashboard.empty.incidents':       'No incidents recorded yet.',
  'dashboard.empty.maintenance':     'No upcoming maintenance scheduled.',
  'dashboard.empty.samples':         'No heartbeats yet — create a monitor to see latency trends.',
  'dashboard.no_match':              'No monitors match the current filter.',
  'dashboard.loading':               'Loading…',
  'dashboard.loading_monitors':      'Loading monitors…',

  // ── dashboard: table column headers ──────────────────────────────
  'dashboard.table.monitor':         'Monitor',
  'dashboard.table.type':            'Type',
  'dashboard.table.p50':             'p50',
  'dashboard.table.last_checks':     'Last 60 checks',
  'dashboard.table.uptime':          'Uptime',

  // ── dashboard: bulk-action toolbar ───────────────────────────────
  'dashboard.bulk.selected':         '{n} selected',
  'dashboard.bulk.pause':            'Pause',
  'dashboard.bulk.resume':           'Resume',
  'dashboard.bulk.channel':          'Channel…',
  'dashboard.bulk.attach':           'Attach',
  'dashboard.bulk.detach':           'Detach',
  'dashboard.bulk.move_to_group':    'Move to group…',
  'dashboard.bulk.ungrouped':        'Ungrouped',
  'dashboard.bulk.delete':           'Delete',
  'dashboard.bulk.clear':            'Clear',
  'dashboard.bulk.delete_confirm':   'Delete {n} monitor(s) and all their heartbeats? This cannot be undone.',

  // ── common ───────────────────────────────────────────────────────
  'common.cancel':                   'Cancel',
  'common.save':                     'Save',
  'common.delete':                   'Delete',
  'common.clear':                    'Clear',

  // ── locale picker ────────────────────────────────────────────────
  'locale.picker.label':             'Language',
};

export default en;
