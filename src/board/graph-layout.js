// The drawing of the board graph: one SVG with a dot for each node, a line from each parent to
// its child and an arrow from each prerequisite to the node that depends_on it. A live force
// layout (graph-force.js) moves the dots, and the camera (graph-camera.js) moves the view.
// - A drag of a dot holds it at the pointer, and the linked dots follow it.
// - A drag from any other place pans the view. A click with no drag picks a dot or clears.
// - A pointer over a dot shows its label and marks its direct neighbours.
// - A dot is larger the higher its level: a root is the largest, a node 3 levels down or
//   deeper the smallest. A lit dot grows by 1.5 and keeps its place in this order.
import {forceSimulation,seedPoints,PARENT_LENGTH,LINK_LENGTH,REST} from 'board/graph-force.js';
import {graphCamera} from 'board/graph-camera.js';
const SVG='http://www.w3.org/2000/svg';
// The dot radius of each level, and the growth of a lit dot.
const RADIUS=[12,8.5,6,4.5],LIT_GROWTH=1.5;
export function svg(tag,attributes={},cls){
  const element=document.createElementNS(SVG,tag);
  for(const [key,value] of Object.entries(attributes))element.setAttribute(key,value);
  if(cls)element.setAttribute('class',cls);
  return element;
}
// Each edge goes from upstream to downstream: {from, to, element, depends}. The canvas goes
// into `holder` before the layout starts, so the frame loop sees it.
export function drawGraph(nodes,onPick,holder){
  const points=seedPoints(nodes);
  const canvas=svg('svg',{role:'img','aria-label':'Graph of the workspace nodes'},'graph-canvas');
  const defs=svg('defs'),marker=svg('marker',{id:'graph-arrow',viewBox:'0 0 10 10',refX:'10',refY:'5',markerWidth:'7',markerHeight:'7',orient:'auto-start-reverse'});
  marker.append(svg('path',{d:'M0 0L10 5L0 10z'},'graph-arrow-head'));defs.append(marker);
  const layers=['graph-parents','graph-arrows','graph-dots','graph-labels'].map(cls=>svg('g',{},cls));
  canvas.append(defs,...layers);
  const edges=[],dotOf=new Map(),labelOf=new Map(),stepOf=new Map(),neighbors=new Map();
  const near=(a,b)=>{for(const [x,y] of [[a,b],[b,a]]){if(!neighbors.has(x))neighbors.set(x,new Set());neighbors.get(x).add(y);}};
  const edge=(from,to,depends)=>{
    if(!points.has(from)||!points.has(to)||from===to)return;
    const element=depends?svg('path',{},'graph-edge graph-depends'):svg('line',{},'graph-edge graph-parent');
    element.dataset.from=from;element.dataset.to=to;layers[depends?1:0].append(element);
    edges.push({from,to,element,depends});near(from,to);
  };
  for(const node of nodes.values()){
    edge(node.parent,node.address,false);
    for(const e of node.relations?.depends_on?.edges||[])edge(e.to,node.address,true);
  }
  for(const node of nodes.values()){
    const tone=node.signal?.tone||'grey',level=Math.min(depth(nodes,node.address),RADIUS.length-1),r=RADIUS[level];
    const p=points.get(node.address);p.baseR=r;p.r=r;p.level=level;p.len=short(node.title||node.address).length;p.px=NaN;p.py=NaN;p.moved=false;
    const dot=svg('circle',{r},'graph-node');dot.dataset.address=node.address;dot.dataset.tone=tone;dot.dataset.level=String(level);dot.style.fill='var(--dot-'+tone+')';
    const title=svg('title');title.textContent=node.title||node.address;dot.append(title);
    const label=svg('text',{},'graph-label graph-label-level-'+level);label.textContent=short(node.title||node.address);label.dataset.address=node.address;
    const step=svg('text',{},'graph-step');step.dataset.address=node.address;
    layers[2].append(dot);layers[3].append(label,step);dotOf.set(node.address,dot);labelOf.set(node.address,label);stepOf.set(node.address,step);
  }
  const camera=graphCamera(canvas);
  let moving=[...points.values()],liveEdges=edges,uScale=1;
  const graph={canvas,edges,dotOf,labelOf,stepOf,neighbors,points,camera,follow:null,sim:null,nearEls:[],candidates:[]};
  const syncScale=u=>{
    uScale=Math.max(0.8,Math.pow(u||1,0.72));
    for(const p of moving){
      p.r=(p.baseR+(p.litGrow||0))*uScale;
      const dot=dotOf.get(p.address);dot.setAttribute('r',p.r.toFixed(1));
      labelOf.get(p.address).setAttribute('x',(p.x+p.r+3*u).toFixed(1));labelOf.get(p.address).setAttribute('y',(p.y-p.r-3*u).toFixed(1));
      stepOf.get(p.address).setAttribute('x',(p.x-p.r-4*u).toFixed(1));stepOf.get(p.address).setAttribute('y',(p.y-p.r-3*u).toFixed(1));
    }
    cullLabels(u||1);
  };
  const cullLabels=u=>{
    const boxes=[];
    for(const p of graph.candidates){
      const label=labelOf.get(p.address);
      if(p.forceLabel){label.classList.add('graph-label-shown');const sx=(p.x+p.r)/u,sy=p.y/u,w=p.len*7+10;boxes.push([sx-4,sy-11,sx+w,sy+9]);continue;}
      // Deeper levels appear as the reader zooms in (smaller u) or in smaller subgraphs.
      if(p.level>=2&&u>1.8&&moving.length>80){label.classList.remove('graph-label-shown');continue;}
      if(p.level>=3&&u>1.1&&moving.length>40){label.classList.remove('graph-label-shown');continue;}
      const sx=(p.x+p.r)/u,sy=p.y/u,w=p.len*(p.level===0?8.5:7)+10;
      const box=[sx-4,sy-11,sx+w,sy+9];
      if(boxes.some(b=>box[0]<b[2]&&box[2]>b[0]&&box[1]<b[3]&&box[3]>b[1])){label.classList.remove('graph-label-shown');continue;}
      boxes.push(box);label.classList.add('graph-label-shown');
    }
  };
  const render=force=>{
    const u=Number(canvas.style.getPropertyValue('--u'))||1;
    for(const p of moving){
      // A point that was never drawn has no last place (NaN), so it counts as moved.
      p.moved=force||!(Math.abs(p.x-p.px)+Math.abs(p.y-p.py)<0.25);
      if(!p.moved)continue;
      p.px=p.x;p.py=p.y;
      const address=p.address,r=p.r,dot=dotOf.get(address);
      dot.setAttribute('cx',p.x.toFixed(1));dot.setAttribute('cy',p.y.toFixed(1));
      labelOf.get(address).setAttribute('x',(p.x+r+3*u).toFixed(1));labelOf.get(address).setAttribute('y',(p.y-r-3*u).toFixed(1));
      stepOf.get(address).setAttribute('x',(p.x-r-4*u).toFixed(1));stepOf.get(address).setAttribute('y',(p.y-r-3*u).toFixed(1));
    }
    for(const {from,to,element,depends} of liveEdges){
      const a=points.get(from),b=points.get(to);
      if(!force&&!a.moved&&!b.moved)continue;
      if(!depends){element.setAttribute('x1',a.x.toFixed(1));element.setAttribute('y1',a.y.toFixed(1));element.setAttribute('x2',b.x.toFixed(1));element.setAttribute('y2',b.y.toFixed(1));continue;}
      // The arrow bends to the left of its direction and ends at the rim of the target dot.
      const cx=(a.x+b.x)/2-(b.y-a.y)*0.15,cy=(a.y+b.y)/2+(b.x-a.x)*0.15,l=Math.hypot(b.x-cx,b.y-cy)||1,r=b.r+2;
      element.setAttribute('d',`M${a.x.toFixed(1)} ${a.y.toFixed(1)}Q${cx.toFixed(1)} ${cy.toFixed(1)} ${(b.x-(b.x-cx)/l*r).toFixed(1)} ${(b.y-(b.y-cy)/l*r).toFixed(1)}`);
    }
    if(force)cullLabels(u);
  };
  const followed=()=>[...(graph.follow||points.keys())].map(a=>points.get(a)).filter(Boolean);
  // The canvas names the state of the layout in data-state: running or rest.
  const tick=alpha=>{if(!canvas.isConnected)return false;render(false);if(alpha<=REST){if(graph.follow!==false)camera.fit(followed());cullLabels(Number(canvas.style.getPropertyValue('--u'))||1);}canvas.dataset.state=alpha>REST?'running':'rest';return true;};
  // Run the layout on the `active` nodes only, or on every node. `layer` gives the flow layer.
  graph.run=(active,layer)=>{
    graph.sim?.stop();
    moving=[...points.values()].filter(p=>!active||active.has(p.address));
    liveEdges=edges.filter(e=>!active||(active.has(e.from)&&active.has(e.to)));
    const springs=liveEdges.map(e=>{
      const target=points.get(e.to),len=e.depends?LINK_LENGTH:(target.level===1?110:target.level===2?48:24);
      return {source:points.get(e.from),target,length:len,depends:e.depends};
    });
    graph.sim=forceSimulation(moving,springs,tick,layer&&(p=>layer.pos?.get(p.address)||null));
    if(matchMedia('(prefers-reduced-motion: reduce)').matches){graph.sim.settle();if(graph.follow!==false)camera.fit(followed());}
    else{graph.sim.warm(65);render(true);if(graph.follow!==false)camera.fit(followed());canvas.dataset.state='running';graph.sim.heat(0.16);}
  };
  // A lit dot grows by LIT_GROWTH over the radius of its level.
  graph.grow=(address,lit)=>{const dot=dotOf.get(address),p=points.get(address);if(dot&&p){p.litGrow=lit?LIT_GROWTH:0;p.r=(p.baseR+p.litGrow)*uScale;dot.setAttribute('r',p.r.toFixed(1));}};
  // The camera follows a set of nodes, or every node, until the reader moves the view.
  graph.focus=set=>{graph.follow=set;camera.fit(followed());};
  graph.cull=()=>cullLabels(Number(canvas.style.getPropertyValue('--u'))||1);
  camera.onMove=()=>{graph.follow=false;};
  camera.onView=syncScale;
  pointer(graph,onPick);
  holder.prepend(canvas);graph.run(null,null);
  return graph;
}
// The number of parents above a node in the graph. A parent cycle stops the count.
function depth(nodes,address){
  const seen=new Set([address]);let level=0;
  for(let up=nodes.get(address)?.parent;up&&nodes.has(up)&&!seen.has(up);up=nodes.get(up).parent){seen.add(up);level++;}
  return level;
}
function short(text){return text.length>40?text.slice(0,39)+'…':text;}
function pointer(graph,onPick){
  const {canvas,camera,points}=graph;let drag=null;
  canvas.addEventListener('pointerdown',event=>{
    if(event.button!==0)return;
    // No text selection and no native drag start from inside the graph.
    event.preventDefault();try{canvas.setPointerCapture(event.pointerId);}catch{}
    const dot=event.target.closest?.('.graph-node');
    drag={x:event.clientX,y:event.clientY,last:[event.clientX,event.clientY],moved:false,point:dot?points.get(dot.dataset.address):null};
    camera.stop();hover(graph,null);
  });
  canvas.addEventListener('pointermove',event=>{
    if(!drag){const dot=event.target.closest?.('.graph-node');hover(graph,dot?dot.dataset.address:null);return;}
    if(Math.hypot(event.clientX-drag.x,event.clientY-drag.y)>3)drag.moved=true;
    if(!drag.moved)return;
    if(drag.point){const [x,y]=camera.point(event);drag.point.fx=x;drag.point.fy=y;graph.follow=false;graph.sim?.heat(0.25);}
    else{camera.pan(drag.last[0]-event.clientX,drag.last[1]-event.clientY);graph.follow=false;}
    drag.last=[event.clientX,event.clientY];
  });
  const end=event=>{
    if(!drag)return;const {point,moved}=drag;drag=null;
    if(point){point.fx=point.fy=null;if(moved)graph.sim?.heat(0.08);}
    if(!moved&&event.type==='pointerup')onPick(point?point.address:null);
  };
  canvas.addEventListener('pointerup',end);canvas.addEventListener('pointercancel',end);
  canvas.addEventListener('pointerleave',()=>{if(!drag)hover(graph,null);});
}
// Mark the dot under the pointer, its direct neighbours and the edges between them.
function hover(graph,address){
  if(graph.hovered===address)return;graph.hovered=address;
  for(const element of graph.nearEls)element.classList.remove('graph-near');
  graph.nearEls=[];
  if(!address)return;
  const mark=el=>{if(el){el.classList.add('graph-near');graph.nearEls.push(el);}};
  const near=new Set([address,...(graph.neighbors.get(address)||[])]);
  for(const a of near){mark(graph.dotOf.get(a));mark(graph.labelOf.get(a));}
  for(const e of graph.edges)if((e.from===address&&near.has(e.to))||(e.to===address&&near.has(e.from)))mark(e.element);
}
