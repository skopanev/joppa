// The entry module: it starts a snapshot or restores the browser session.
import {state} from 'board/state.js';
import {$,E} from 'board/dom.js';
import {notify} from 'board/feedback.js';
import {showApp} from 'board/sign-in.js';
import {load} from 'board/board.js';
// The theme switch has no exports. The import runs it.
import 'board/theme.js';
if (window.SNAPSHOT) {
  document.body.classList.remove('checking-session');
  document.body.classList.add('snapshot');
  $('login-screen').remove();
  $('refresh').remove();
  $('timeline-link').hidden=true;
  document.body.classList.add('authenticated');
  state.data = window.SNAPSHOT;
  state.ws = state.data.workspace;
  const option=E('option',state.data.state.title);option.value=state.ws;$('workspaces').replaceChildren(option);
  load().catch(notify);
}

// Browser sessions use an HttpOnly cookie; no token field or token URL.
if(!window.SNAPSHOT){
  sessionStorage.removeItem('joppa-token');
  if(location.hash.startsWith('#token='))history.replaceState(null,'',location.pathname+location.search);
  fetch('/api/me').then(async r=>{
    if(r.ok)await showApp(await r.json());
    else if(r.status!==401)throw Error('Could not restore your session. Please reload the page.');
  }).catch(e=>{$('login-status').textContent=e.message;$('login-status').dataset.error='true';})
    .finally(()=>document.body.classList.remove('checking-session'));
}
