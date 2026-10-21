// The API client. A snapshot page reads its embedded objects instead of the server.
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
  if(!window.SNAPSHOT)return api('workspaces/'+encodeURIComponent(state.ws)+'/objects?'+new URLSearchParams(params));
  const objects=state.data.snapshot_objects||{};
  if(params.address){
    const object=objects[params.address];if(!object)throw Error('Unknown object');
    const document=object.revisions.find(r=>r.revision===Number(params.revision||object.current_revision));if(!document)throw Error('Unknown revision');
    const historical=document.revision!==object.current_revision;
    const links=document.links.map(link=>({link,title:objects[link.target]?.revisions.at(-1).data.title,current_revision:objects[link.target]?.current_revision}));
    return {...object,document,historical,links,annotations:historical?{}:object.annotations,process:historical?null:object.process};
  }
  const presentParents=new Set(Object.values(objects).filter(o=>o.revisions.at(-1).present).map(o=>o.revisions.at(-1).parent));
  const items=Object.values(objects).filter(o=>{
    const r=o.revisions.at(-1);
    const related=params.related_to&&(r.parent===params.related_to||r.links.some(l=>l.target===params.related_to)||objects[params.related_to]?.revisions.at(-1).links.some(l=>l.target===o.address));
    return (r.present||presentParents.has(o.address))&&(!params.kind||params.kind===o.kind)&&(params.related_to?related:params.parent==='*'||(params.parent||null)===r.parent)&&(!params.note_status||(r.data.status||'unprocessed')===params.note_status);
  }).map(o=>{const r=o.revisions.at(-1);return {...o,parent:r.parent,title:r.data.title,owner:r.data.owner,revision:r.revision,at:r.at,present:r.present,note_status:r.data.status};});
  return {items,next_cursor:null};
}
