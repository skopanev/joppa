// The layout and the drawing of the board graph. The parent edges make a radial tree: every
// node without children sits on the outer ring at an equal step, and a parent sits on the ring
// of its depth at the mean angle of its leaves. The same nodes give the same places.
const SVG='http://www.w3.org/2000/svg';
// The ring step and the arc between two neighbour leaves, in SVG units.
const RING=110,LEAF_ARC=16;
export function svg(tag,attributes={},cls){
  const element=document.createElementNS(SVG,tag);
  for(const [key,value] of Object.entries(attributes))element.setAttribute(key,value);
  if(cls)element.setAttribute('class',cls);
  return element;
}
// The place of each node: {x, y, depth}. A node whose parent is not in the graph is a root.
export function radialLayout(nodes){
  const children=new Map(),roots=[];
  for(const node of nodes.values()){
    const parent=node.parent&&node.parent!==node.address&&nodes.has(node.parent)?node.parent:null;
    if(parent){if(!children.has(parent))children.set(parent,[]);children.get(parent).push(node.address);}
    else roots.push(node.address);
  }
  for(const list of children.values())list.sort();roots.sort();
  // The leaves in tree order, and the depth of each node. A node is placed once.
  const leaves=[],depth=new Map(),order=[];
  const walk=(address,level)=>{
    if(depth.has(address))return;depth.set(address,level);order.push(address);
    const below=children.get(address)||[];
    if(!below.length)leaves.push(address);
    for(const child of below)walk(child,level+1);
  };
  for(const address of roots)walk(address,0);
  const deepest=Math.max(0,...depth.values());
  const outer=Math.max((deepest+1)*RING,leaves.length*LEAF_ARC/(2*Math.PI));
  const angle=new Map(leaves.map((address,i)=>[address,2*Math.PI*i/Math.max(1,leaves.length)]));
  // A parent takes the mean angle of its leaves: the children come after it in tree order.
  for(const address of [...order].reverse()){
    const below=children.get(address)||[];
    if(below.length)angle.set(address,below.reduce((sum,child)=>sum+angle.get(child),0)/below.length);
  }
  const places=new Map();
  for(const address of order){
    const leaf=!(children.get(address)||[]).length;
    const radius=leaf?outer:outer*(depth.get(address)+1)/(deepest+1);
    const a=angle.get(address);
    places.set(address,{x:radius*Math.cos(a),y:radius*Math.sin(a),depth:depth.get(address)});
  }
  return {places,outer};
}
// The drawing: a group of parent edges, a group of depends_on arrows and a group of dots.
// Each dot has the colour of its status tone and the title of its node.
export function drawGraph(nodes,layout,onPick){
  const pad=60,size=2*(layout.outer+pad);
  const canvas=svg('svg',{viewBox:[-size/2,-size/2,size,size].join(' '),role:'img','aria-label':'Graph of the workspace nodes'},'graph-canvas');
  const defs=svg('defs');const marker=svg('marker',{id:'graph-arrow',viewBox:'0 0 10 10',refX:'10',refY:'5',markerWidth:'7',markerHeight:'7',orient:'auto-start-reverse'});
  marker.append(svg('path',{d:'M0 0L10 5L0 10z'},'graph-arrow-head'));defs.append(marker);
  const parents=svg('g',{},'graph-parents'),arrows=svg('g',{},'graph-arrows'),dots=svg('g',{},'graph-dots'),labels=svg('g',{},'graph-labels');
  canvas.append(defs,parents,arrows,dots,labels);
  const edges=[],dotOf=new Map(),labelOf=new Map();
  const at=address=>layout.places.get(address);
  for(const node of nodes.values()){
    const from=at(node.parent),to=at(node.address);
    if(!from||!to||node.parent===node.address)continue;
    const line=svg('line',{x1:from.x,y1:from.y,x2:to.x,y2:to.y},'graph-edge graph-parent');
    parents.append(line);edges.push({from:node.parent,to:node.address,element:line});
  }
  for(const node of nodes.values())for(const link of node.links||[]){
    if(link.relation!=='depends_on'||!at(link.target)||link.target===node.address)continue;
    const from=at(node.address),to=at(link.target);
    // The arrow bends to the left of its direction and ends at the rim of the target dot.
    const dx=to.x-from.x,dy=to.y-from.y,r=radius(to)+2;
    const cx=(from.x+to.x)/2-dy*0.2,cy=(from.y+to.y)/2+dx*0.2;
    const ex=to.x-(to.x-cx)/Math.hypot(to.x-cx,to.y-cy)*r,ey=to.y-(to.y-cy)/Math.hypot(to.x-cx,to.y-cy)*r;
    const path=svg('path',{d:`M${from.x} ${from.y}Q${cx} ${cy} ${ex} ${ey}`,'marker-end':'url(#graph-arrow)'},'graph-edge graph-depends');
    path.dataset.from=node.address;path.dataset.to=link.target;
    arrows.append(path);edges.push({from:node.address,to:link.target,element:path,depends:true});
  }
  for(const node of nodes.values()){
    const place=at(node.address);if(!place)continue;
    const tone=node.signal?.tone||'grey';
    const dot=svg('circle',{cx:place.x,cy:place.y,r:radius(place)},'graph-node');
    dot.dataset.address=node.address;dot.dataset.tone=tone;dot.style.fill='var(--dot-'+tone+')';
    const title=svg('title');title.textContent=node.title||node.address;dot.append(title);
    dot.addEventListener('click',event=>{event.stopPropagation();onPick(node.address);});
    dots.append(dot);dotOf.set(node.address,dot);
    const label=svg('text',{x:place.x+radius(place)+3,y:place.y+4},'graph-label');label.textContent=short(node.title||node.address);
    label.dataset.address=node.address;labels.append(label);labelOf.set(node.address,label);
  }
  canvas.addEventListener('click',()=>onPick(null));
  panAndZoom(canvas,size);
  return {canvas,edges,dotOf,labelOf};
}
function radius(place){return place.depth===0?7:4.5;}
function short(text){return text.length>40?text.slice(0,39)+'…':text;}
// The wheel zooms around the pointer, and a drag moves the view.
function panAndZoom(canvas,size){
  let view=canvas.getAttribute('viewBox').split(' ').map(Number),drag=null,moved=false;
  const set=()=>canvas.setAttribute('viewBox',view.join(' '));
  const point=event=>{const box=canvas.getBoundingClientRect();return [view[0]+(event.clientX-box.left)/box.width*view[2],view[1]+(event.clientY-box.top)/box.height*view[3]];};
  canvas.addEventListener('wheel',event=>{
    event.preventDefault();const [x,y]=point(event),k=Math.exp(Math.sign(event.deltaY)*0.15);
    const width=Math.min(size*2,Math.max(size/40,view[2]*k)),scale=width/view[2];
    view=[x-(x-view[0])*scale,y-(y-view[1])*scale,width,view[3]*scale];set();
  },{passive:false});
  canvas.addEventListener('pointerdown',event=>{
    if(event.target!==canvas)return;drag={x:event.clientX,y:event.clientY,view:[...view]};moved=false;canvas.setPointerCapture?.(event.pointerId);
  });
  canvas.addEventListener('pointermove',event=>{
    if(!drag)return;const box=canvas.getBoundingClientRect();
    const dx=(event.clientX-drag.x)/box.width*view[2],dy=(event.clientY-drag.y)/box.height*view[3];
    if(Math.abs(event.clientX-drag.x)+Math.abs(event.clientY-drag.y)>3)moved=true;
    view=[drag.view[0]-dx,drag.view[1]-dy,view[2],view[3]];set();
  });
  const end=()=>{drag=null;};canvas.addEventListener('pointerup',end);canvas.addEventListener('pointercancel',end);
  // The click after a drag does not clear the picked node.
  canvas.addEventListener('click',event=>{if(moved)event.stopImmediatePropagation();moved=false;},true);
}
