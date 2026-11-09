// The drawing of Flow view: one SVG with a dot for each node and an arrow for each edge, in the
// columns of flow-layout.js. The camera (flow-camera.js) moves the view.
// - A forward edge is a curve from left to right. A back edge is an arc over its two nodes.
// - The condition `when` of an edge shows on the edge. Every label shows.
// - Under the label of a node, one transfer badge names each other relation of the node.
// - A drag from any place pans the view and picks nothing. The dots keep their place.
// - A click with no drag picks a dot or a label, opens a badge, or clears on the canvas.
import {flowCamera} from 'board/flow-camera.js';
import {flowLayout} from 'board/flow-layout.js';
const SVG='http://www.w3.org/2000/svg';
// The dot radius, the space of a curve from its dots, and the text lines under a dot.
const RADIUS=9,GAP=3,LABEL=RADIUS+19,BADGE=20;
// The first view uses at most MOST world units for one pixel, so its text is readable. The
// text sizes of src/board_more.css grow with --u up to the same limit.
const MOST=1.6;
function svg(tag,attributes={},cls){
  const element=document.createElementNS(SVG,tag);
  for(const [key,value] of Object.entries(attributes))element.setAttribute(key,value);
  if(cls)element.setAttribute('class',cls);
  return element;
}
const at=n=>n.toFixed(1);
// `nodes` come in address order: {address, tone, label, badges: [{key, title}]}. Each edge is
// {from, to, when} in its stored order. `on` gives pick(address) and transfer(key, address).
// The canvas goes into `holder` before the fit, so the camera can read its box.
export function drawFlow(holder,nodes,edges,on){
  const {position}=flowLayout(nodes.map(n=>n.address),edges);
  const canvas=svg('svg',{role:'img','aria-label':'Flow of the relation'},'flow-canvas');
  const defs=svg('defs'),marker=svg('marker',{id:'flow-arrow',viewBox:'0 0 10 10',refX:'10',refY:'5',markerWidth:'7',markerHeight:'7',orient:'auto-start-reverse'});
  marker.append(svg('path',{d:'M0 0L10 5L0 10z'},'flow-arrow-head'));defs.append(marker);
  const [lines,whens,dots,labels]=['flow-edges','flow-whens','flow-dots','flow-labels'].map(cls=>svg('g',{},cls));
  canvas.append(defs,lines,whens,dots,labels);
  // parts: the dot, the label and the badges of each node, which take the same marks.
  const parts=new Map();
  for(const node of nodes){
    const {x,y}=position.get(node.address);
    const dot=svg('circle',{cx:at(x),cy:at(y),r:RADIUS},'flow-node');
    dot.dataset.address=node.address;dot.dataset.tone=node.tone;dot.style.fill='var(--dot-'+node.tone+')';
    const title=svg('title');title.textContent=node.label;dot.append(title);
    const label=svg('text',{x:at(x),y:at(y+LABEL)},'flow-label');label.textContent=short(node.label);label.dataset.address=node.address;
    const badges=node.badges.map((badge,i)=>{
      const element=svg('text',{x:at(x),y:at(y+LABEL+BADGE*(i+1))},'flow-transfer');
      element.textContent='⇄ '+badge.title;element.dataset.relation=badge.key;element.dataset.address=node.address;
      return element;
    });
    dots.append(dot);labels.append(label,...badges);parts.set(node.address,[dot,label,...badges]);
  }
  const drawn=edges.map(edge=>{
    const a=position.get(edge.from),b=position.get(edge.to);
    const {d,mid}=edge.back?arc(a,b):curve(a,b);
    const element=svg('path',{d,'marker-end':'url(#flow-arrow)'},'flow-edge'+(edge.back?' flow-back':''));
    element.dataset.from=edge.from;element.dataset.to=edge.to;lines.append(element);
    let label=null;
    if(edge.when){label=svg('text',{x:at(mid.x),y:at(mid.y-6)},'flow-when');label.textContent=short(edge.when);whens.append(label);}
    return {...edge,element,label,mid};
  });
  holder.prepend(canvas);
  // The view shows every dot and the top of every arc.
  const tops=drawn.filter(e=>e.back).map(e=>e.mid);
  const camera=flowCamera(canvas);camera.fit([...position.values(),...tops],undefined,MOST);
  pointer(canvas,camera,on);
  return {canvas,camera,position,parts,edges:drawn};
}
// A forward edge leaves the right of its dot and ends at the left of the target dot.
function curve(a,b){
  const sx=a.x+RADIUS+GAP,ex=b.x-RADIUS-GAP,d=(ex-sx)/2;
  return {d:`M${at(sx)} ${at(a.y)}C${at(sx+d)} ${at(a.y)} ${at(ex-d)} ${at(b.y)} ${at(ex)} ${at(b.y)}`,mid:{x:(sx+ex)/2,y:(a.y+b.y)/2}};
}
// A back edge leaves the top of its dot and comes down on the top of the target dot. Its
// highest point is over both dots. An edge to its own node makes a loop.
function arc(a,b){
  const sy=a.y-RADIUS-GAP,ey=b.y-RADIUS-GAP,top=Math.min(sy,ey)-70-Math.abs(a.x-b.x)*0.25;
  const spread=a.address===b.address?40:0;
  const d=`M${at(a.x)} ${at(sy)}C${at(a.x+spread)} ${at(top)} ${at(b.x-spread)} ${at(top)} ${at(b.x)} ${at(ey)}`;
  return {d,mid:{x:(a.x+b.x)/2,y:(sy+ey+6*top)/8}};
}
function short(text){return text.length>36?text.slice(0,35)+'…':text;}
function pointer(canvas,camera,on){
  let drag=null;
  canvas.addEventListener('pointerdown',event=>{
    if(event.button!==0)return;
    // No text selection and no native drag start from inside the drawing.
    event.preventDefault();try{canvas.setPointerCapture(event.pointerId);}catch{}
    drag={x:event.clientX,y:event.clientY,last:[event.clientX,event.clientY],moved:false,target:event.target};
  });
  canvas.addEventListener('pointermove',event=>{
    if(!drag)return;
    if(Math.hypot(event.clientX-drag.x,event.clientY-drag.y)>3)drag.moved=true;
    if(!drag.moved)return;
    camera.pan(drag.last[0]-event.clientX,drag.last[1]-event.clientY);
    drag.last=[event.clientX,event.clientY];
  });
  const end=event=>{
    if(!drag)return;const {moved,target}=drag;drag=null;
    if(moved||event.type!=='pointerup')return;
    const badge=target.closest?.('.flow-transfer');
    if(badge)return on.transfer(badge.dataset.relation,badge.dataset.address);
    on.pick(target.closest?.('[data-address]')?.dataset.address||null);
  };
  canvas.addEventListener('pointerup',end);canvas.addEventListener('pointercancel',end);
}
