// The board load: it reads the selected workspace and draws the page for the address.
import {state} from 'board/state.js';
import {$,E} from 'board/dom.js';
import {notify} from 'board/feedback.js';
import {closeAccountMenu} from 'board/account-menu.js';
import {api} from 'board/api.js';
import {clearBoard} from 'board/workspaces.js';
import {inboxKinds} from 'board/object-types.js';
import {rootCollectionTabs} from 'board/collections.js';
import {renderTimelinePage} from 'board/timeline.js';
import {renderObjectPage} from 'board/object-page.js';
import {commonObjectFilters} from 'board/filters.js';
import {commonNotes} from 'board/notes.js';
export async function load() {
  closeAccountMenu();
  const request = ++state.loadNumber;
  let next;
  // The response and the error of an older selection are not shown.
  try{next = window.SNAPSHOT || (await api('workspaces/' + encodeURIComponent(state.ws)+'/board'));}catch(e){if(request!==state.loadNumber)return;throw e;}
  if (request !== state.loadNumber) return;
  state.data = next;
  state.data.objectTypes=next.object_types||{};
  applyStatuses(next.statuses);
  $('timeline-link').href='/?workspace='+encodeURIComponent(state.ws)+'&view=timeline';
  $('tree-schema-link').href='/tree?workspace='+encodeURIComponent(state.ws);
  $('suggested-method-link').href='/guide?workspace='+encodeURIComponent(state.ws);
  await renderObjectTree();
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
  const root=$('tree');root.replaceChildren();
  const generation=state.loadNumber;
  if(window.SNAPSHOT)state.data.objectTypes=state.data.snapshot_schema.types;
  else if(!state.data.objectTypes||!Object.keys(state.data.objectTypes).length){
    const configuration=await api('workspaces/'+encodeURIComponent(state.ws)+'/tree');if(generation!==state.loadNumber)return;
    state.data.objectTypes=configuration.schema.types;
  }
  const selection=new URLSearchParams(location.search);
  if(selection.has('object'))return renderObjectPage(root,selection.get('object'),selection.get('revision'),generation);
  if(selection.get('view')==='timeline')return renderTimelinePage(root,generation);
  if(state.data.launch.configured)root.append(E('p',state.data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+(state.data.launch.blocker_count??state.data.launch.blockers?.length),'hint'));
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
