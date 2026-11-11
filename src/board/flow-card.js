// The card of the picked node of Flow view, over the canvas. The head gives the collapse button
// first, then the status, the title of the node in the relation, the copy button of its ID, the
// copy button of the link to the node in Flow view, Open and the close button ×. The facts give
// the object title (Object), the address, the status label and the nodes one edge before
// (Previous) and after (Next) the node, each with the condition of its edge. A step picks its
// node. The
// section Facts after Next lists the facts of the step in stored order: the status circle of
// the own tone of each fact, its title, the copy button of its ID and Open.
import {E} from 'board/dom.js';
import {objectURL,flowNodeURL} from 'board/links.js';
import {copyButton,copyLinkButton} from 'board/clipboard.js';
// The collapse button of the head hides every part below the head and keeps the pick and the
// marks. The collapsed state stays for the next pick.
// `flow` gives the picked `address`, its `node` with its `label`, the `relation` key, the
// `edges` of the drawing, its `facts` [{address, title, tone, item_id}], `nameOf(address)` and
// `toneOf(address)` (the dot tone) of every node, the `ownTone` of the node, `pick`,
// `collapsed` and `collapse(on)`.
export function flowCard(card,flow){
  const {address,node,label,relation,edges,facts:listed=[],nameOf,toneOf,ownTone,pick,collapsed=false,collapse}=flow;
  card.replaceChildren();card.hidden=!address;card.dataset.address=address||'';
  if(!address)return card.classList.remove('flow-card-collapsed');
  const close=E('button','×','flow-card-close');close.type='button';close.setAttribute('aria-label','Close the card');close.onclick=()=>pick(null);
  const open=E('a','Open →','flow-open');open.href=objectURL(address);
  const fold=E('button',undefined,'flow-card-collapse');fold.type='button';
  const head=E('div',undefined,'flow-card-head');
  head.append(fold,E('span',undefined,'status-circle '+toneOf(address)),E('h3',label,'flow-card-title'),copyButton(node.item_id||address),
    copyLinkButton(flowNodeURL(relation,address)),open,close);
  const facts=E('dl',undefined,'flow-card-facts');
  // The toggle changes only the card: no pick and no marks.
  const show=on=>{
    card.classList.toggle('flow-card-collapsed',on);facts.hidden=on;
    fold.textContent=on?'▸':'▾';fold.setAttribute('aria-label',on?'Expand the card':'Collapse the card');fold.setAttribute('aria-expanded',String(!on));
  };
  fold.onclick=()=>{const on=!card.classList.contains('flow-card-collapsed');collapse?.(on);show(on);};
  show(collapsed);
  const fact=(term,...values)=>{const dd=E('dd');dd.append(...values);facts.append(E('dt',term),dd);};
  fact('Object',node.title||address);fact('Address',address);fact('Status',node.signal?.label||ownTone||toneOf(address));
  const steps=(list,other)=>list.length?list.map(e=>step(e[other],e.when,nameOf,toneOf,pick)):[E('span','none','hint')];
  fact('Previous',...steps(edges.filter(e=>e.to===address),'from'));
  fact('Next',...steps(edges.filter(e=>e.from===address),'to'));
  if(listed.length)fact('Facts',...listed.map(factRow));
  card.append(head,facts);
}
function factRow({address,title,tone,item_id}){
  const row=E('div',undefined,'flow-fact');row.dataset.address=address;
  const open=E('a','Open →','flow-open');open.href=objectURL(address);
  row.append(E('span',undefined,'status-circle '+(tone||'blue')),E('span',title||address,'flow-fact-title'),copyButton(item_id||address),open);
  return row;
}
function step(address,when,nameOf,toneOf,pick){
  const button=E('button',undefined,'flow-step');button.type='button';button.dataset.address=address;
  button.append(E('span',undefined,'status-circle '+toneOf(address)),nameOf(address));
  if(when)button.append(E('small',when,'flow-step-when hint'));
  button.onclick=()=>pick(address);
  return button;
}
