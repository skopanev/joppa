// The API client.
import {state} from 'board/state.js';
import {$,E} from 'board/dom.js';
export async function api(path) {
  // A 404 changes the board only while the request still belongs to the shown selection.
  const generation=state.loadNumber,selected=state.ws;
  const r = await fetch('/api/' + path);
  // A proxy error page or a cut reply is not JSON. Report the HTTP status, not a parser error.
  const text = await r.text();
  let j;
  try{j=text?JSON.parse(text):{};}catch{j={error:'The server sent an unreadable reply (HTTP '+r.status+'). Try again.'};if(r.ok)throw Error(j.error);}
  if(r.status===401){state.loadNumber++;state.data=null;document.body.classList.remove('authenticated');$('tree').replaceChildren();$('login-status').textContent='Your session expired. Please sign in again.';}
  if(r.status===404 && path.startsWith('workspaces/')&&!path.includes('/objects?')&&generation===state.loadNumber&&selected===state.ws){state.data=null;$('tree').replaceChildren(E('p','This workspace is no longer available. Choose another workspace or ask its owner for access.','empty-state'));}
  if(path.includes('/objects?')&&(r.status===404||j.error==='unknown object'))throw Error('Object not found.');
  if (!r.ok) throw Error(j.error || JSON.stringify(j));
  return j;
}
/* Common objects use the same cards for every configured type. */
export async function objectPart(params){
  return api('workspaces/'+encodeURIComponent(state.ws)+'/objects?'+new URLSearchParams(params));
}
