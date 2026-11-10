// The drawing of Flow view: one SVG with a dot for each node and an arrow for each edge, in the
// columns of flow-layout.js. The camera (flow-camera.js) moves the view.
// - A forward edge is a curve from left to right through its waypoints. A back edge is an arc
//   over its two nodes.
// - A node label wraps by words into at most 3 lines (wrapLabel of flow-layout.js). The dot
//   title keeps the full label.
// - The condition `when` of an edge stands in one line on the first part of the edge, and
//   touches no other condition. Every label shows.
// - Under the label of a node, one transfer badge names each other relation of the node.
// - A coloured edge (flow-color.js) has its stroke and an arrow head marker of its colour. An
//   uncoloured edge keeps the shared marker #flow-arrow.
// - A drag from any place pans the view and picks nothing. The dots keep their place.
// - A click with no drag picks a dot or a label, opens a badge, or clears on the canvas.
import {flowCamera} from 'board/flow-camera.js';
import {flowLayout,wrapLabel,apartWhens} from 'board/flow-layout.js';
import {flowColors,paint,colorMarker} from 'board/flow-color.js';
const SVG='http://www.w3.org/2000/svg';
// The dot radius, the space of a curve from its dots, and the first label line under a dot.
const RADIUS=9,GAP=3,LABEL=RADIUS+19;
// The text sizes of .flow-label and .flow-transfer in src/board_more.css, the line height in
// em, the space from the last label baseline to the first badge baseline, and the badge step
// in badge em.
const LABEL_SIZE=13,BADGE_SIZE=11,LINE_HEIGHT=1.2,FIRST_BADGE=17,BADGE_STEP=1.4;
// The first view shows the whole flow, centred. FIT_MARGIN leaves room for the labels and
// badges under the lowest row; the person zooms in from there.
const FIT_MARGIN=90;
function svg(tag,attributes={},cls){
  const element=document.createElementNS(SVG,tag);
  for(const [key,value] of Object.entries(attributes))element.setAttribute(key,value);
  if(cls)element.setAttribute('class',cls);
  return element;
}
const at=n=>n.toFixed(1);
// `nodes` come in address order: {address, tone, label, color, badges: [{key, title}]}. Each
// edge is {from, to, when, color} in its stored order. `on` gives pick(address) and
// transfer(key, address).
// The canvas goes into `holder` before the fit, so the camera can read its box.
export function drawFlow(holder,nodes,edges,on){
  // Edge order: the sources in address order, then the stored order.
  const index=new Map(nodes.map((n,i)=>[n.address,i]));
  edges=[...edges].sort((a,b)=>index.get(a.from)-index.get(b.from));
  const {position,column}=flowLayout(nodes.map(n=>n.address),edges);
  flowColors(nodes,edges,column);
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
    // One tspan for each line of the label, each next line one line height lower.
    const label=svg('text',{x:at(x),y:at(y+LABEL)},'flow-label');label.dataset.address=node.address;
    const rows=wrapLabel(node.label);
    label.append(...rows.map((row,i)=>{const line=svg('tspan',{x:at(x),...(i?{dy:LINE_HEIGHT+'em'}:{})});line.textContent=row;return line;}));
    // The badges go below the last line. Their offsets are in the em of the badge, so they grow
    // with the label text (--k of src/board_more.css).
    const below=((rows.length-1)*LINE_HEIGHT*LABEL_SIZE+FIRST_BADGE)/BADGE_SIZE;
    const badges=node.badges.map((badge,i)=>{
      const element=svg('text',{x:at(x),y:at(y+LABEL),dy:(below+i*BADGE_STEP).toFixed(3)+'em'},'flow-transfer');
      element.textContent='⇄ '+badge.title;element.dataset.relation=badge.key;element.dataset.address=node.address;
      return element;
    });
    dots.append(dot);labels.append(label,...badges);parts.set(node.address,[dot,label,...badges]);
  }
  const drawn=edges.map(edge=>{
    const a=position.get(edge.from),b=position.get(edge.to);
    const {d,mid}=edge.back?arc(a,b):curve(a,b,edge.via);
    const color=paint(edge.color),head=color?colorMarker(defs,marker,edge.color):'flow-arrow';
    const element=svg('path',{d,'marker-end':'url(#'+head+')'},'flow-edge'+(edge.back?' flow-back':'')+(color?' flow-colored':''));
    if(color)element.style.stroke=color;
    element.dataset.from=edge.from;element.dataset.to=edge.to;lines.append(element);
    return {...edge,element,label:null,mid};
  });
  // A condition stands 6 above the middle of the first part of its edge, and then moves apart
  // from the earlier conditions (apartWhens of flow-layout.js).
  const asked=drawn.filter(e=>e.when);
  apartWhens(asked.map(e=>({x:e.mid.x,y:e.mid.y-6,text:short(e.when)}))).forEach((place,i)=>{
    const label=svg('text',{x:at(place.x),y:at(place.y)},'flow-when');label.textContent=place.text;
    whens.append(label);asked[i].label=label;
  });
  holder.prepend(canvas);
  // The view shows every dot, every waypoint and the top of every arc.
  const tops=drawn.filter(e=>e.back).map(e=>e.mid),via=drawn.flatMap(e=>e.via||[]);
  const camera=flowCamera(canvas);camera.fit([...position.values(),...via,...tops],FIT_MARGIN);
  pointer(canvas,camera,on);
  return {canvas,camera,position,parts,edges:drawn};
}
// A forward edge leaves the right of its dot, goes through its waypoints and ends at the left of
// the target dot. Each part is a curve that is level at both ends, so the path stays smooth.
// `mid` is the middle of the first part, where the condition of the edge stands.
function curve(a,b,via=[]){
  const points=[{x:a.x+RADIUS+GAP,y:a.y},...via,{x:b.x-RADIUS-GAP,y:b.y}];
  let d=`M${at(points[0].x)} ${at(points[0].y)}`;
  for(let i=1;i<points.length;i++){
    const p=points[i-1],q=points[i],h=(q.x-p.x)/2;
    d+=`C${at(p.x+h)} ${at(p.y)} ${at(q.x-h)} ${at(q.y)} ${at(q.x)} ${at(q.y)}`;
  }
  return {d,mid:{x:(points[0].x+points[1].x)/2,y:(points[0].y+points[1].y)/2}};
}
// A back edge leaves the top of its dot and comes down on the top of the target dot. Its
// highest point is over both dots. An edge to its own node makes a loop.
function arc(a,b){
  const sy=a.y-RADIUS-GAP,ey=b.y-RADIUS-GAP,top=Math.min(sy,ey)-70-Math.abs(a.x-b.x)*0.25;
  const spread=a.address===b.address?40:0;
  const d=`M${at(a.x)} ${at(sy)}C${at(a.x+spread)} ${at(top)} ${at(b.x-spread)} ${at(top)} ${at(b.x)} ${at(ey)}`;
  return {d,mid:{x:(a.x+b.x)/2,y:(sy+ey+6*top)/8}};
}
// The one line of an edge condition.
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
