// The page of one object revision.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectPart} from 'board/api.js';
import {objectURL,objectLink} from 'board/links.js';
import {commonObjectCard} from 'board/card.js';
export async function renderObjectPage(root,address,revision,generation){
  const navigation=E('nav',undefined,'object-page-navigation');navigation.setAttribute('aria-label','Object navigation');
  const board=E('a','Back to board','text-button');board.href=objectURL();navigation.append(board);root.append(navigation);
  const page=E('section',undefined,'object-page');root.append(page);page.append(E('p','Loading object…','hint'));
  try{
    if(!address)throw Error('Object not found.');
    if(revision!==null&&!/^[1-9]\d*$/.test(revision))throw Error('Invalid object revision.');
    const detail=await objectPart(revision===null?{address}:{address,revision});if(generation!==state.loadNumber)return;
    const r=detail.document,d=r.data;
    document.title=d.title+' · Joppa';
    navigation.append(E('span','Revision '+r.revision+(detail.historical?' · Saved document':''),'hint'));
    if(r.parent)navigation.append(objectLink('Parent',r.parent));
    const card=commonObjectCard({...detail,title:d.title,owner:d.owner,revision:r.revision,parent:r.parent,present:r.present,at:r.at,note_status:d.status},detail);
    page.replaceChildren(card);card.open=true;
  }catch(e){if(generation===state.loadNumber&&page.isConnected)page.replaceChildren(E('p',e.message,'empty-state'));}
}
