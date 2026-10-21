// Object rows of a parent or a collection, with paging.
import {state} from 'board/state.js';
import {$,E} from 'board/dom.js';
import {notify,button} from 'board/feedback.js';
import {objectPart} from 'board/api.js';
import {applyObjectColor} from 'board/object-types.js';
import {rootCollectionTabs} from 'board/collections.js';
import {commonObjectCard} from 'board/card.js';
import {applyTestStatuses} from 'board/criterion-tests.js';
export async function appendObjectChildren(root,parent,kind,extra={}){
  const generation=state.loadNumber;
  const params={limit:'50',...extra};if(parent)params.parent=parent;if(kind)params.kind=kind;
  const part=await objectPart(params);if(generation!==state.loadNumber||!root.isConnected)return;
  root.querySelector(':scope > .object-loading')?.remove();
  appendObjectPage(root,part,params,generation);
}
export function appendObjectPage(root,part,params,generation){
  const items=part.items;
  const ancestors=new Set();
  for(let node=root;node;node=node.parentElement){
    const address=node.dataset.objectAddress||node.dataset.objectContextAddress;
    if(address)ancestors.add(address);
  }
  for(const item of items){
    if(ancestors.has(item.address))continue;
    // A retired Task is history: the timeline and its own page keep it, the tree does not.
    if(item.process?.status==='retired')continue;
    let holder=root;
    if(!params.parent&&!params.kind){
      holder=[...root.querySelectorAll(':scope > .object-collection')].find(group=>group.dataset.kind===item.kind);
      if(!holder){
        holder=E('section',undefined,'object-collection root-collection');holder.dataset.kind=item.kind;holder.dataset.collectionKind=item.kind;applyObjectColor(holder,item.kind);
        holder.append(E('h2',state.data.objectTypes[item.kind].title,'object-collection-title'));root.append(holder);
      }
    }
    if(![...holder.querySelectorAll(':scope > .common-object')].some(card=>card.dataset.objectAddress===item.address)){
      // A load or a reload shows the top level only: no branch opens from an earlier visit.
      const card=commonObjectCard(item,undefined,root.closest('.common-object')?.dataset.objectTitle);holder.append(card);
    }
  }
  applyTestStatuses(items);
  if(!items.length&&!part.next_cursor&&!params.parent&&!root.querySelector('.common-object')){
    const type=params.kind&&state.data.objectTypes[params.kind];
    root.append(E('p',type?'No '+type.title.toLowerCase()+' here.':'No objects here yet.','hint collection-empty'));
  }
  if(part.next_cursor){const more=button('Load more',async()=>{more.disabled=true;try{const next=await objectPart({...params,cursor:part.next_cursor});if(generation!==state.loadNumber||!root.isConnected)return;more.remove();appendObjectPage(root,next,params,generation);}catch(e){more.disabled=false;notify(e);}},'text-button');root.append(more);}
  if(!params.parent&&!params.kind&&$('tree').querySelector('.collection-tabs'))rootCollectionTabs($('tree'));
}
