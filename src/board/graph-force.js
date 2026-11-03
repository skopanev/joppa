// The live force layout of the board graph. Every node pushes the other nodes away, and a
// Barnes-Hut quadtree keeps this near n log n. Every edge pulls its two nodes to its length
// like a spring, so children cluster round their parent. A weak pull keeps the graph at the
// centre. The heat (alpha) falls each frame, and the frame loop stops when the graph is at
// rest. A drag holds its node at the pointer and heats the graph again. With `layerX`, each
// node is also pulled to the x of its flow layer, so a path reads from left to right.
const CHARGE=-320,THETA2=0.81,GRAVITY=0.025,FRICTION=0.6,DECAY=0.92,LAYER_PULL=0.82;
// Below this heat the graph is at rest.
export const REST=0.004;
// The spring length of a parent edge and of a depends_on edge.
export const PARENT_LENGTH=60,LINK_LENGTH=160;
// `points` are {address, x, y, vx, vy, fx, fy}. `springs` are {source, target, length} with
// two points. `tick` draws the points and gives false when the graph is gone. `layerX(p)`
// gives the x of the flow layer of a point, or null.
export function forceSimulation(points,springs,tick,targetOf){
  const degree=new Map();
  for(const s of springs)for(const p of [s.source,s.target])degree.set(p,(degree.get(p)||0)+1);
  const edges=springs.map(s=>{
    const a=degree.get(s.source),b=degree.get(s.target);
    const scale=targetOf?(s.depends?0.25:0.08):(s.depends?0.15:1);
    return {...s,strength:scale/Math.min(a,b||1),bias:a/(a+b||1)};
  });
  let alpha=1,frame=0;
  const step=()=>{
    const tree=quadtree(points);
    for(const p of points)repel(tree,p,alpha);
    for(const {source:a,target:b,length,strength,bias} of edges){
      let dx=b.x+b.vx-a.x-a.vx,dy=b.y+b.vy-a.y-a.vy,l=Math.hypot(dx,dy)||1e-6;
      l=(l-length)/l*alpha*strength;dx*=l;dy*=l;
      b.vx-=dx*bias;b.vy-=dy*bias;a.vx+=dx*(1-bias);a.vy+=dy*(1-bias);
    }
    for(const p of points){
      if(p.fx!=null){p.x=p.fx;p.y=p.fy;p.vx=p.vy=0;continue;}
      const target=targetOf?.(p);
      if(target){
        p.vx=(p.vx+(target[0]-p.x)*LAYER_PULL*alpha)*FRICTION;
        p.vy=(p.vy+(target[1]-p.y)*LAYER_PULL*alpha)*FRICTION;
      }else{
        p.vx=(p.vx-p.x*GRAVITY*alpha)*FRICTION;
        p.vy=(p.vy-p.y*GRAVITY*alpha)*FRICTION;
      }
      p.x+=p.vx;p.y+=p.vy;
    }
    collide(points);
    alpha*=DECAY;
  };
  const loop=()=>{frame=0;step();if(tick(alpha)!==false&&alpha>REST)frame=requestAnimationFrame(loop);};
  return {
    // Run `count` steps in memory before drawing, so large graphs appear already clustered.
    warm(count){for(let i=0;i<count&&alpha>REST;i++)step();},
    // Heat the graph to at least `value` and run the frame loop.
    heat(value){alpha=Math.max(alpha,value);if(!frame)frame=requestAnimationFrame(loop);},
    // Run the layout to rest at once, with no frames: for reduced motion.
    settle(){while(alpha>REST)step();tick(alpha);},
    stop(){cancelAnimationFrame(frame);frame=0;},
  };
}
// The first places: the roots on a circle, and each child near its parent. A hash of the
// address gives the offset, so the same nodes start at the same places.
export function seedPoints(nodes){
  const children=new Map(),roots=[];
  for(const node of nodes.values()){
    const parent=node.parent&&node.parent!==node.address&&nodes.has(node.parent)?node.parent:null;
    if(parent){if(!children.has(parent))children.set(parent,[]);children.get(parent).push(node.address);}else roots.push(node.address);
  }
  roots.sort();
  const points=new Map(),ring=40*Math.sqrt(roots.length);
  const place=(address,x,y)=>{
    if(points.has(address))return;points.set(address,{address,x,y,vx:0,vy:0,fx:null,fy:null});
    for(const child of (children.get(address)||[]).sort()){
      const a=2*Math.PI*hash(child),r=PARENT_LENGTH*(0.5+hash(child+'#'));
      place(child,x+r*Math.cos(a),y+r*Math.sin(a));
    }
  };
  roots.forEach((address,i)=>place(address,ring*Math.cos(2*Math.PI*i/roots.length),ring*Math.sin(2*Math.PI*i/roots.length)));
  return points;
}
// Two dots never overlap: a pair closer than its two radii and GAP moves apart. A grid of
// cells as large as the largest such distance finds the near pairs in linear time.
const GAP=6;
function collide(points){
  let size=0;for(const p of points)size=Math.max(size,2*(p.r||5)+GAP);
  const grid=new Map(),key=(x,y)=>x+' '+y;
  points.forEach((p,i)=>{p.cell=[Math.floor(p.x/size),Math.floor(p.y/size)];p.index=i;const k=key(...p.cell);if(!grid.has(k))grid.set(k,[]);grid.get(k).push(p);});
  for(const p of points)for(let i=-1;i<=1;i++)for(let j=-1;j<=1;j++)for(const q of grid.get(key(p.cell[0]+i,p.cell[1]+j))||[]){
    if(q.index<=p.index)continue;
    let dx=q.x-p.x,dy=q.y-p.y,d=Math.hypot(dx,dy);
    if(d<1e-3){dx=hash(q.address)-0.5;dy=hash(q.address+'#')-0.5;d=Math.hypot(dx,dy)||1;}
    const min=(p.r||5)+(q.r||5)+GAP;
    if(d>=min)continue;
    const push=(min-d)/d/((p.fx==null)+(q.fx==null)||1);
    if(p.fx==null){p.x-=dx*push;p.y-=dy*push;}
    if(q.fx==null){q.x+=dx*push;q.y+=dy*push;}
  }
}
function hash(text){let h=2166136261;for(const c of text)h=Math.imul(h^c.charCodeAt(0),16777619);return (h>>>0)/4294967296;}
function quadtree(points){
  let x0=Infinity,y0=Infinity,x1=-Infinity,y1=-Infinity;
  for(const p of points){x0=Math.min(x0,p.x);y0=Math.min(y0,p.y);x1=Math.max(x1,p.x);y1=Math.max(y1,p.y);}
  const root=cell(x0,y0,Math.max(x1-x0,y1-y0,1));
  for(const p of points)insert(root,p,0);
  return root;
}
function cell(x0,y0,size){return {x0,y0,size,mass:0,cx:0,cy:0,kids:null,points:null};}
function insert(c,p,depth){
  c.cx=(c.cx*c.mass+p.x)/(c.mass+1);c.cy=(c.cy*c.mass+p.y)/(c.mass+1);c.mass++;
  // A leaf holds its points. Points at the same place stay in one leaf.
  if(!c.kids&&(!c.points||depth>24||c.points.every(q=>q.x===p.x&&q.y===p.y))){(c.points||=[]).push(p);return;}
  if(!c.kids){c.kids=[null,null,null,null];const old=c.points;c.points=null;for(const q of old)into(c,q,depth);}
  into(c,p,depth);
}
function into(c,p,depth){
  const h=c.size/2,i=(p.x>=c.x0+h?1:0)+(p.y>=c.y0+h?2:0);
  c.kids[i]||=cell(c.x0+(i&1?h:0),c.y0+(i&2?h:0),h);
  insert(c.kids[i],p,depth+1);
}
function repel(c,p,alpha){
  if(!c?.mass)return;
  const dx=c.cx-p.x,dy=c.cy-p.y,d2=dx*dx+dy*dy;
  if(c.kids&&c.size*c.size>=THETA2*d2){for(const k of c.kids)repel(k,p,alpha);return;}
  // A far cell pushes as one mass at its centre. A node does not push itself.
  const mass=c.kids?c.mass:c.points.filter(q=>q!==p).length;
  if(!mass)return;
  if(d2<1e-6){p.vx+=(hash(p.address)-0.5);p.vy+=(hash(p.address+'#')-0.5);return;}
  const w=CHARGE*mass*alpha/d2;p.vx+=dx*w;p.vy+=dy*w;
}
