// The card of the picked node of Flow view, over the canvas. The head gives the status, the
// title of the node in the relation, the copy button of its ID, Open and the close button ×. The facts give the object
// title (Object), the address, the status label and the nodes one edge before (Previous) and
// after (Next) the node, each with the condition of its edge. A step picks its node.
import {E} from 'board/dom.js';
import {objectURL} from 'board/links.js';
import {copyButton} from 'board/clipboard.js';
// `flow` gives the picked `address`, its `node` with its `label`, the `edges` of the drawing,
// `nameOf(address)` and `toneOf(address)` of every node, and `pick`.
export function flowCard(card,flow){
  const {address,node,label,edges,nameOf,toneOf,pick}=flow;
  card.replaceChildren();card.hidden=!address;card.dataset.address=address||'';
  if(!address)return;
  const close=E('button','×','flow-card-close');close.type='button';close.setAttribute('aria-label','Close the card');close.onclick=()=>pick(null);
  const open=E('a','Open →','flow-open');open.href=objectURL(address);
  const head=E('div',undefined,'flow-card-head');
  head.append(E('span',undefined,'status-circle '+toneOf(address)),E('h3',label,'flow-card-title'),copyButton(node.item_id||address),open,close);
  const facts=E('dl',undefined,'flow-card-facts');
  const fact=(term,...values)=>{const dd=E('dd');dd.append(...values);facts.append(E('dt',term),dd);};
  fact('Object',node.title||address);fact('Address',address);fact('Status',node.signal?.label||toneOf(address));
  const steps=(list,other)=>list.length?list.map(e=>step(e[other],e.when,nameOf,toneOf,pick)):[E('span','none','hint')];
  fact('Previous',...steps(edges.filter(e=>e.to===address),'from'));
  fact('Next',...steps(edges.filter(e=>e.from===address),'to'));
  card.append(head,facts);
}
function step(address,when,nameOf,toneOf,pick){
  const button=E('button',undefined,'flow-step');button.type='button';button.dataset.address=address;
  button.append(E('span',undefined,'status-circle '+toneOf(address)),nameOf(address));
  if(when)button.append(E('small',when,'flow-step-when hint'));
  button.onclick=()=>pick(address);
  return button;
}
