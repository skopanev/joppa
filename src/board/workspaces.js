// The workspace list and the workspace choice.
import {state} from 'board/state.js';
import {$,E} from 'board/dom.js';
import {notify} from 'board/feedback.js';
import {api} from 'board/api.js';
import {load} from 'board/board.js';
import {objectURL} from 'board/links.js';
// Until the workspace list arrives, the drop-down shows the last opened title, not a placeholder.
try{const title=sessionStorage.getItem('joppa-workspace-title');if(title)$('workspaces').append(E('option',title));}catch{}
export async function list() {
  const list = await api('workspaces');
  $('workspaces').replaceChildren();
  // Выбирать не из чего — значит и выбора нет. При одном воркспейсе список
  // остаётся видимым (человек должен знать, где он), но перестаёт быть
  // элементом управления: щелчок по нему не обещает того, чего не случится.
  $('workspaces').disabled = list.length <= 1;
  if (!list.length) {const o=E('option','No workspaces yet');o.value='';$('workspaces').append(o);}
  for (const w of list) {
    const o = E('option', w.title);
    o.value = w.id;
    $('workspaces').append(o);
  }
  const requested=new URLSearchParams(location.search).get('workspace');
  if(requested&&!list.some(w=>w.id===requested)){
    state.ws='';state.data=null;$('workspaces').value='';
    const link=E('a','Back to board','text-button');const url=new URL(location.href);url.search='';url.hash='';link.href=url.href;
    $('tree').replaceChildren(E('p','This workspace is not available to your account.','empty-state'),link);return;
  }
  if (!list.some(w => w.id === state.ws)) state.ws = list.find(w=>w.id===sessionStorage.getItem('joppa-workspace'))?.id || list[0]?.id || '';
  $('workspaces').value = state.ws;
  // Back to the first entry returns to the workspace that the list chose for it.
  if(state.ws&&!requested)history.replaceState({...history.state,workspace:state.ws},'',location.href);
  try{sessionStorage.setItem('joppa-workspace-title',list.find(w=>w.id===state.ws)?.title||'');}catch{}
  if (state.ws) await load();
  else {state.data=null;$('tree').replaceChildren(E('h1','No workspace access'),E('p','Ask your workspace owner to grant access.'));}
}
/// The rows of the old selection go away at once, so nobody can use them under the new selection.
export function clearBoard(){$('tree').replaceChildren(E('p','Loading workspace…','object-loading hint'));}
$('workspaces').onchange=e=>{
  state.ws=e.target.value;sessionStorage.setItem('joppa-workspace',state.ws);clearBoard();
  try{sessionStorage.setItem('joppa-workspace-title',e.target.selectedOptions[0]?.textContent||'');}catch{}
  history.pushState(null,'',objectURL());
  load().catch(notify);
};
