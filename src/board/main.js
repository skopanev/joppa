// The entry module: it restores the browser session.
import {$} from 'board/dom.js';
import {showApp} from 'board/sign-in.js';
// The theme switch has no exports. The import runs it.
import 'board/theme.js';
// Browser sessions use an HttpOnly cookie; no token field or token URL.
sessionStorage.removeItem('joppa-token');
if(location.hash.startsWith('#token='))history.replaceState(null,'',location.pathname+location.search);
fetch('/api/me').then(async r=>{
  if(r.ok)await showApp(await r.json());
  else if(r.status!==401)throw Error('Could not restore your session. Please reload the page.');
}).catch(e=>{$('login-status').textContent=e.message;$('login-status').dataset.error='true';})
  .finally(()=>document.body.classList.remove('checking-session'));
