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
  // ── dashboard ────────────────────────────────────────────────────
  'dashboard.title':                 'Dashboard',
  'dashboard.search_placeholder':    'Search monitors…',
  'dashboard.add_monitor':           'Add monitor',
  'dashboard.status_page':           'Status page',
  'dashboard.sign_out':              'Sign out',
  'dashboard.uptime_24h':            '24h uptime',
  'dashboard.response_time':         'Response time',
  'dashboard.recent_incidents':      'Recent incidents',
  'dashboard.view_all':              'View all',
  'dashboard.maintenance':           'Maintenance',
  'dashboard.all_monitors':          'All monitors',
  'dashboard.empty.title':           'No monitors yet',
  'dashboard.empty.cta':             'Add your first monitor',
  'dashboard.empty.incidents':       'No incidents recorded yet.',
  'dashboard.empty.maintenance':     'No upcoming maintenance scheduled.',
  'dashboard.empty.samples':         'No heartbeats yet — create a monitor to see latency trends.',
  'dashboard.no_match':              'No monitors match the current filter.',
  'dashboard.loading':               'Loading…',

  // ── common ───────────────────────────────────────────────────────
  'common.cancel':                   'Cancel',
  'common.save':                     'Save',
  'common.delete':                   'Delete',
  'common.clear':                    'Clear',
};

export default en;
