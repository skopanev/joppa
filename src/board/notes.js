// Inbox collections and related input on an object.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {notify} from 'board/feedback.js';
import {applyObjectColor,inputQueue} from 'board/object-types.js';
import {collectionHeading} from 'board/collections.js';
import {appendObjectChildren} from 'board/object-list.js';
export function commonNotes(root,relatedTo,kind){
  const label=state.data.objectTypes?.[kind]?.title||kind;
  const title=relatedTo?'Related '+label.toLowerCase():label;
  const queue=inputQueue(kind);
  const section=E('section');section.append(E('header',undefined,'collection-heading'));root.append(section);section.classList.add('note-section');applyObjectColor(section,kind);
  collectionHeading(section,title,state.data.objectTypes?.[kind]?.description||(!relatedTo&&queue?'Incoming information to review.':''));
  if(!relatedTo){section.classList.add('root-collection');section.dataset.collectionKind=kind;}
  const summary=section.querySelector(':scope > header');
  const body=E('div',undefined,'note-section-body');section.append(body);
  let loaded=false,loading=false,request=0;
  let all;
  if(!relatedTo&&queue){
    const controls=E('div',undefined,'note-controls');
    const option=E('label',undefined,'note-all');all=E('input');all.type='checkbox';all.setAttribute('aria-label','All '+label.toLowerCase());option.append(all,E('span','All'));controls.append(option);summary.append(controls);
  }
  const rows=E('div',undefined,'note-items');body.append(rows);
  const loadNotes=async()=>{
    const mine=++request;loaded=false;loading=true;
    const holder=E('div');holder.append(E('p','Loading '+label.toLowerCase()+'…','object-loading hint'));rows.replaceChildren(holder);
    try{await appendObjectChildren(holder,'*',kind,relatedTo?{related_to:relatedTo}:queue&&!all?.checked?{note_status:'unprocessed'}:{});if(mine===request&&section.isConnected){
      loaded=true;const empty=!holder.querySelector('.common-object');section.classList.toggle('empty-notes',empty);if(relatedTo)section.hidden=empty;
      summary.title=empty?'No '+label.toLowerCase()+' here.':'Read '+label.toLowerCase();
    }}
    catch(e){if(mine===request){loaded=false;holder.replaceChildren();notify(e);}}
    finally{if(mine===request)loading=false;}
  };
  if(all)all.onchange=loadNotes;
  section.collectionLoad=()=>{if(!loaded&&!loading)loadNotes();};
  if(relatedTo){
    const parent=section.parentElement.closest('details');
    if(!parent||parent.open)section.collectionLoad();
    else parent.addEventListener('toggle',()=>{if(parent.open)section.collectionLoad();});
  }
  return section;
}
