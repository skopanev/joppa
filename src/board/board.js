// The board load: it reads the selected workspace and draws the page for the address.
import {state} from 'board/state.js';
import {$,E} from 'board/dom.js';
import {notify} from 'board/feedback.js';
import {closeAccountMenu} from 'board/account-menu.js';
import {api} from 'board/api.js';
import {clearBoard} from 'board/workspaces.js';
import {inboxKinds} from 'board/object-types.js';
import {rootCollectionTabs} from 'board/collections.js';
import {renderFlowPage,closeFlowPage} from 'board/flow.js';
import {forgetRelations} from 'board/relations.js';
import {renderObjectPage} from 'board/object-page.js';
import {commonObjectFilters,LINK_FILTERS} from 'board/filters.js';
import {commonNotes} from 'board/notes.js';
export async function load() {
  closeAccountMenu();forgetRelations();
  const request = ++state.loadNumber;
  let next;
  // The response and the error of an older selection are not shown.
  try{next = await api('workspaces/' + encodeURIComponent(state.ws)+'/board');}catch(e){if(request!==state.loadNumber)return;throw e;}
  if (request !== state.loadNumber) return;
  state.data = next;
  state.data.objectTypes=next.object_types||{};
  applyStatuses(next.statuses);
  // The view drop-down switches between Tree and Flow and keeps the node filters of the link.
  // A change reads the filters of that moment. Any other view in the link opens the tree.
  const selection=new URLSearchParams(location.search),viewSelect=$('view-select');
  viewSelect.value=selection.get('view')==='flow'?'flow':'tree';
  viewSelect.onchange=()=>{location.href=viewHref(viewSelect.value);};
  $('tree-schema-link').href='/tree?workspace='+encodeURIComponent(state.ws);
  $('suggested-method-link').href='/guide?workspace='+encodeURIComponent(state.ws);
  await renderObjectTree();
}
function viewHref(name){
  const now=new URLSearchParams(location.search),next=new URLSearchParams({workspace:state.ws});
  if(name!=='tree')next.set('view',name);
  for(const key of LINK_FILTERS)if(now.get(key))next.set(key,now.get(key));
  return '/?'+next;
}
// The tree schema gives each status tone a light and a dark colour. The theme picks one.
function applyStatuses(statuses){
  const style=document.documentElement.style;
  for(const status of Array.isArray(statuses)?statuses:[]){
    if(!/^[a-z]+$/.test(status?.tone||''))continue;
    for(const theme of ['light','dark']){
      if(/^#[0-9a-f]{6}$/i.test(status[theme]||''))style.setProperty('--dot-'+status.tone+'-'+theme,status[theme]);
    }
  }
}
async function renderObjectTree(){
  const root=$('tree');closeFlowPage();root.replaceChildren();
  const generation=state.loadNumber;
  if(!state.data.objectTypes||!Object.keys(state.data.objectTypes).length){
    const configuration=await api('workspaces/'+encodeURIComponent(state.ws)+'/tree');if(generation!==state.loadNumber)return;
    state.data.objectTypes=configuration.schema.types;
  }
  const selection=new URLSearchParams(location.search);
  if(selection.has('object'))return renderObjectPage(root,selection.get('object'),selection.get('revision'),generation);
  if(selection.get('view')==='flow')return renderFlowPage(root,generation);
  const contents=E('div',undefined,'object-filter-results');contents.hidden=true;
  commonObjectFilters(root,contents);root.append(contents);
  for(const kind of inboxKinds().filter(kind=>state.data.objectTypes[kind].root))commonNotes(root,undefined,kind);
  rootCollectionTabs(root);
}
window.addEventListener('popstate',()=>{
  closeAccountMenu();
  // The first entry has no workspace in its address. Its history state names the workspace.
  const next=new URLSearchParams(location.search).get('workspace')||history.state?.workspace||state.ws;
  if(next&&next!==state.ws){state.ws=next;$('workspaces').value=state.ws;clearBoard();}
  if(state.ws)load().catch(notify);
});
$('refresh').onclick = () => load().catch(notify);
