// German — real translations for high-traffic keys; everything else
// falls back to the English source via the `...en` spread. See en.js for
// the rationale behind the flat dotted-key layout.
import en from './en.js';

const de = {
  ...en,
  'dashboard.title':        'Übersicht',
  'dashboard.add_monitor':  'Monitor hinzufügen',
  'dashboard.status_page':  'Statusseite',
  'dashboard.sign_out':     'Abmelden',
  'dashboard.all_monitors': 'Alle Monitore',
  'dashboard.loading':      'Wird geladen…',
  'common.cancel':          'Abbrechen',
  'common.save':            'Speichern',
  'common.delete':          'Löschen',
  'common.clear':           'Zurücksetzen',
  'locale.picker.label':    'Sprache',
};

export default de;
