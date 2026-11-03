// The card of the picked node of the board graph, over the canvas. It shows the title, the
// address, the status with its label and the readiness, the parent, the data fields and the
// start of the body, with Open and a close button. The steps of the card go one hop along the
// depends_on links: the nodes that the node depends on, the node, and the nodes that depend on
// it. A step or the parent picks that node in the graph.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectPart} from 'board/api.js';
import {objectURL} from 'board/links.js';
import {personLabel} from 'board/format.js';
// The card shows this many characters of the body, and this many characters of a field value.
const BODY=280,VALUE=120;
let collapsed=false;
export function nodeCard(card,graph,nodes,address,pick,onFocus){
  card.replaceChildren();card.hidden=!address;
  if(!address)return;
  const node=nodes.get(address)||{address},generation=state.loadNumber;card.dataset.address=address;
  card.classList.toggle('graph-card-collapsed',collapsed);
  const close=E('button','×','graph-card-close');close.type='button';close.setAttribute('aria-label','Close the card');close.onclick=()=>pick(null);
  const toggle=E('button',collapsed?'Details ▾':'Hide ▴','graph-hop');toggle.type='button';
  toggle.onclick=()=>{collapsed=!collapsed;card.classList.toggle('graph-card-collapsed',collapsed);toggle.textContent=collapsed?'Details ▾':'Hide ▴';};
  const focusBtn=E('button','Focus flow','graph-hop');focusBtn.type='button';
  if(onFocus)focusBtn.onclick=()=>onFocus(address);
  const open=E('a','Open →','graph-open');open.href=objectURL(address);
  const tone=node.signal?.tone||'grey',head=E('div',undefined,'graph-card-head');
  head.append(E('span',undefined,'status-circle '+tone),E('strong',node.title||address,'graph-card-title'), ...(onFocus?[focusBtn]:[]),toggle,open,close);
  const details=E('div',undefined,'graph-card-details');
  const facts=E('dl',undefined,'graph-card-facts');
  const fact=(term,value)=>{if(value===undefined||value===null||value==='')return;facts.append(E('dt',term),value instanceof Node?wrap(value):E('dd',String(value)));};
  fact('Address',address);fact('Status',node.signal?.label||tone);
  if(node.parent)fact('Parent',hop(nodes,node.parent,pick));
  const body=E('p',undefined,'graph-card-body hint');
  details.append(facts,body,steps(graph,nodes,address,pick));
  card.append(head,details);
  // The detail read gives the readiness, the data fields and the body.
  objectPart({address}).then(detail=>{
    if(generation!==state.loadNumber||card.dataset.address!==address||!card.isConnected)return;
    const data=detail.document?.data||{},mark=detail.process?.assessment;
    if(detail.work_ready===false||mark?.state==='waiting')fact('Readiness',mark?.state==='waiting'?'Waits: '+mark.reason:'Waits for '+(detail.waits_for?.title||'a dependency'));
    for(const [key,value] of Object.entries(data)){
      if(['title','body'].includes(key))continue;
      fact(key,key==='owner'||key==='assignee'?personLabel(value):short(Array.isArray(value)?value.join(', '):typeof value==='object'?JSON.stringify(value):String(value),VALUE));
    }
    body.textContent=short(String(data.body||''),BODY);body.hidden=!data.body;
  }).catch(e=>{if(card.dataset.address===address)body.textContent=e.message;});
}
function wrap(element){const dd=E('dd');dd.append(element);return dd;}
function short(text,length){return text.length>length?text.slice(0,length-1)+'…':text;}
function hop(nodes,address,pick){
  const node=nodes.get(address),button=E('button',undefined,'graph-hop');button.type='button';button.dataset.address=address;
  button.append(E('span',undefined,'status-circle '+(node?.signal?.tone||'grey')),node?.title||address);
  button.onclick=()=>pick(address);return button;
}
// Upstream → the node → downstream, one hop along the depends_on links.
function steps(graph,nodes,address,pick){
  const up=graph.edges.filter(e=>e.depends&&e.to===address).map(e=>e.from),down=graph.edges.filter(e=>e.depends&&e.from===address).map(e=>e.to);
  const row=E('div',undefined,'graph-card-steps');
  const group=(title,list)=>{const part=E('div',undefined,'graph-card-step');part.append(E('span',title,'graph-card-step-title hint'));for(const a of list)part.append(hop(nodes,a,pick));if(!list.length)part.append(E('span','none','hint'));return part;};
  const here=E('div',undefined,'graph-card-step graph-card-here');here.append(E('span','This node','graph-card-step-title hint'),E('strong',nodes.get(address)?.title||address));
  row.append(group('Depends on',up),E('span','→','graph-trail-arrow'),here,E('span','→','graph-trail-arrow'),group('Needed by',down));
  return row;
}
