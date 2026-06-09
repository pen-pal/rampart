// French — real translations for high-traffic keys; everything else
// falls back to the English source via the `...en` spread. See en.js for
// the rationale behind the flat dotted-key layout.
import en from './en.js';

const fr = {
  ...en,
  'dashboard.title':        'Tableau de bord',
  'dashboard.add_monitor':  'Ajouter un moniteur',
  'dashboard.status_page':  'Page de statut',
  'dashboard.sign_out':     'Se déconnecter',
  'dashboard.all_monitors': 'Tous les moniteurs',
  'dashboard.loading':      'Chargement…',
  'common.cancel':          'Annuler',
  'common.save':            'Enregistrer',
  'common.delete':          'Supprimer',
  'common.clear':           'Effacer',
  'locale.picker.label':    'Langue',
};

export default fr;
