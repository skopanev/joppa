// The timeline page of a workspace.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {notify,button} from 'board/feedback.js';
import {api} from 'board/api.js';
import {personLabel,eventDate} from 'board/format.js';
import {objectTitlePreview} from 'board/object-types.js';
import {objectURL} from 'board/links.js';
/// The workspace timeline, newest change first. A row with an object revision opens that revision.
export async function renderTimelinePage(root,generation){
  const navigation=E('nav',undefined,'object-page-navigation');navigation.setAttribute('aria-label','Timeline navigation');
  const board=E('a','Back to board','text-button');board.href=objectURL();navigation.append(board);
  const page=E('section',undefined,'timeline-page');page.append(E('h2','Timeline','timeline-title'));
  const list=E('ol',undefined,'timeline-list');page.append(list);root.append(navigation,page);
  document.title='Timeline · Joppa';
  const more=async cursor=>{
    const params=new URLSearchParams({order:'newest',limit:'50'});if(cursor)params.set('cursor',cursor);
    const part=await api('workspaces/'+encodeURIComponent(state.ws)+'/timeline?'+params);if(generation!==state.loadNumber||!page.isConnected)return;
    page.querySelector(':scope > .timeline-more')?.remove();
    for(const item of part.items)list.append(timelineRow(item));
    if(!list.children.length)page.append(E('p','No changes yet.','hint'));
    if(part.next_cursor){const next=button('Load older changes',async()=>{next.disabled=true;try{await more(part.next_cursor);}catch(e){next.disabled=false;notify(e);}},'text-button timeline-more');page.append(next);}
  };
  try{await more();}catch(e){if(generation===state.loadNumber&&page.isConnected)page.append(E('p',e.message,'empty-state'));}
}
function timelineRow(item){
  const row=E('li',undefined,'timeline-entry');row.dataset.position=item.position;row.dataset.change=item.change;
  const kind=item.kind||String(item.address||'').split(':')[0];
  const title=item.title||item.address||item.op;
  const target=item.address&&item.revision?E('a',undefined,'timeline-row'):E('div',undefined,'timeline-row');
  if(target.tagName==='A'){target.href=objectURL(item.address,item.revision);target.dataset.linkAddress=item.address;}
  const time=E('time',eventDate(item.at),'timeline-time');time.dateTime=item.at;
  target.append(time,E('span',personLabel(item.actor),'timeline-actor'),E('span',objectTitlePreview(title,kind),'timeline-object'),E('span',item.change,'timeline-change'));
  target.title=[item.change,title,item.reason].filter(Boolean).join(' · ');
  row.append(target);return row;
}
