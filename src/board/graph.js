// The graph view of the board (view=graph): every present node with its parent edges and its
// depends_on arrows, in a live force layout.
// - A large graph shows the top levels only (Detail depth). Its roots without a link hide.
// - A filter, a search, a field or a root keeps its subgraph: the matches, their branch and
//   their depends_on nodes up and down. The other nodes and edges hide.
// - The flow of a clicked dot, or of a small subgraph, is lit: its depends_on nodes up to
//   2 hops away (1 hop from more than 3 starts), with step numbers in flow layers, a trail
//   and a card (graph-card.js). The other nodes fade almost out.
// - Focus flow of the card keeps only the flow of its node. A click on the canvas resets it.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectPart} from 'board/api.js';
import {inboxKinds} from 'board/object-types.js';
import {commonObjectFilters,matches} from 'board/filters.js';
import {drawGraph} from 'board/graph-layout.js';
import {nodeCard} from 'board/graph-card.js';
// The graph reads lean pages of the server maximum: a path needs every node.
const NODE_PAGE='100',SEARCH_PAGE='50',FIELDS='kind,parent,title,signal,relations';
// The trail lists this many steps at most. A subgraph of at most PATH_MAX nodes shows its
// flow at once. A graph of more than LARGE nodes shows the levels up to DEFAULT_LEVEL first.
// Flow layers are LAYER_GAP apart, and the nodes of one layer ROW_GAP apart.
const TRAIL=40,PATH_MAX=24,LARGE=120,DEFAULT_LEVEL=2,ALL_LEVELS=Infinity,LAYER_GAP=300,ROW_GAP=150;
export async function renderGraphPage(root,generation){
  document.title='Graph · Joppa';
  const page=E('section',undefined,'graph-page');
  const live=()=>generation===state.loadNumber&&page.isConnected;
  const status=E('p','Loading nodes…','graph-status hint');status.setAttribute('role','status');
  const trail=E('nav',undefined,'graph-picked');trail.setAttribute('aria-label','Path steps');trail.hidden=true;
  const view=E('div',undefined,'graph-view'),card=E('aside',undefined,'graph-card');card.hidden=true;card.setAttribute('aria-label','Picked node');
  const ui={graph:null,nodes:null,found:null,filtered:null,focused:false,click:null,maxLevel:DEFAULT_LEVEL,active:'',status,trail,card};
  const paint=()=>{if(ui.graph)light(ui);};
  // A click on the picked dot again, on the canvas or on × clears the pick.
  ui.pick=address=>{ui.click=address===ui.click?null:address;if(!ui.click&&ui.focused){ui.found=ui.filtered;ui.focused=false;}paint();};
  const filters=commonObjectFilters(root,E('div'),async({params,text,scope,filtering,current})=>{
    // Detail depth is the depth node filter: depth N keeps the levels below N.
    ui.maxLevel=params.depth?Number(params.depth)-1:DEFAULT_LEVEL;
    const clean={...params};delete clean.depth;
    const hasFilter=filtering&&Object.keys(clean).length>0||Boolean(text);
    const next=hasFilter?await matchSet(ui.nodes,text,clean,scope,current):null;
    if(next===undefined||!current())return;
    ui.found=ui.filtered=next;ui.focused=false;ui.click=null;paint();
  });
  page.append(status,trail,view);root.append(page);
  const nodes=new Map();let cursor;
  try{
    do{
      const part=await objectPart({parent:'*',limit:NODE_PAGE,fields:FIELDS,...(cursor?{cursor}:{})});
      if(!live())return;
      for(const item of part.items)nodes.set(item.address,item);
      status.textContent='Loading nodes… '+nodes.size+' of '+part.total;cursor=part.next_cursor;
    }while(cursor);
  }catch(e){if(live())status.textContent=e.message;return;}
  if(!nodes.size){status.textContent='No objects here yet.';return;}
  // The root picker offers root domains and their capabilities right underneath.
  const inbox=new Set(inboxKinds()),kidsOf=new Map();
  for(const n of nodes.values())if(n.parent){if(!kidsOf.has(n.parent))kidsOf.set(n.parent,[]);kidsOf.get(n.parent).push(n);}
  const allRoots=[...nodes.values()].filter(n=>!n.parent&&!inbox.has(n.kind)).sort((a,b)=>(a.title||'').localeCompare(b.title||''));
  const roots=allRoots.some(n=>kidsOf.has(n.address))?allRoots.filter(n=>kidsOf.has(n.address)):allRoots;
  const rootOptions=[['','All roots']];
  for(const r of roots){
    rootOptions.push([r.address,r.title||r.address]);
    for(const c of (kidsOf.get(r.address)||[]).filter(k=>kidsOf.has(k.address)).sort((a,b)=>(a.title||'').localeCompare(b.title||''))){
      rootOptions.push([c.address,'  ↳ '+(c.title||c.address)]);
    }
  }
  filters.offer('parent','Object root',rootOptions);
  filters.offer('depth','Detail depth',[['','Top '+(DEFAULT_LEVEL+1)+' levels'],['2','Top 2 levels'],['100','All levels']]);
  ui.focusFlow=address=>{ui.found=new Set([address]);ui.focused=true;ui.click=address;paint();};
  view.append(card);ui.nodes=nodes;ui.graph=drawGraph(nodes,ui.pick,view);paint();
}
// The addresses that match the filters. Pure parent/kind filters run in memory at once.
async function matchSet(nodes,text,params,scope,current){
  const keys=Object.keys(params);
  if(nodes&&!text&&keys.every(k=>k==='parent'||k==='kind')){
    const found=new Set();
    for(const n of nodes.values())if((!params.kind||n.kind===params.kind)&&(!params.parent||n.address===params.parent||n.parent===params.parent))found.add(n.address);
    return found;
  }
  const list=text?{...params,q:text,limit:SEARCH_PAGE,...scope}:{parent:'*',...params,limit:NODE_PAGE,fields:'kind'};
  const found=new Set();let cursor;
  if(!text&&params.parent&&params.parent!=='*'&&!params.kind&&!params.status&&!params.ready&&!params.owner&&!params.data)found.add(params.parent);
  do{
    const part=await matches(text,cursor?{...list,cursor}:list);if(!current())return undefined;
    for(const item of part.items){
      if(!text)found.add(item.address);
      else if(item.workspace===state.ws&&!item.historical)found.add(item.kind+':'+item.id);
    }
    cursor=text?null:part.next_cursor;
  }while(cursor);
  return found;
}
// The subtree of `seeds`, and when `expand` is true (or the subtree has at most PATH_MAX nodes)
// every node connected to it by depends_on upstream or downstream.
export function chain(seeds,edges,within,expand=true){
  const down=new Map(),up=new Map(),kids=new Map(),parent=new Map();
  const add=(map,key,value)=>{if(!map.has(key))map.set(key,[]);map.get(key).push(value);};
  for(const e of edges){
    if(within&&!(within.has(e.from)&&within.has(e.to)))continue;
    if(e.depends){add(down,e.from,e.to);add(up,e.to,e.from);}else{add(kids,e.from,e.to);parent.set(e.to,e.from);}
  }
  const reach=(start,next)=>{const seen=new Set(start),queue=[...start];while(queue.length)for(const o of next.get(queue.pop())||[])if(!seen.has(o)){seen.add(o);queue.push(o);}return seen;};
  const branch=reach(seeds,kids);
  if(!expand&&branch.size>PATH_MAX)return branch;
  const lit=new Set([...reach(branch,down),...reach(branch,up)]);
  if(expand&&branch.size<=PATH_MAX)for(const address of [...lit])for(let p=parent.get(address);p&&!lit.has(p);p=parent.get(p))lit.add(p);
  return lit;
}
// The flow layer of each node of `set`: 0 for a node with no upstream edge inside the set, else
// one more than its highest upstream node. A node in a cycle takes the layer after the nodes
// before it. `max` is the highest layer.
export function flowLayers(set,edges){
  const into=new Map([...set].map(a=>[a,0])),out=new Map(),layer=new Map();
  for(const e of edges)if(set.has(e.from)&&set.has(e.to)){into.set(e.to,into.get(e.to)+1);if(!out.has(e.from))out.set(e.from,[]);out.get(e.from).push(e.to);}
  const queue=[...set].filter(a=>!into.get(a)).sort();
  for(const a of queue)layer.set(a,0);
  for(;;){
    while(queue.length){const a=queue.shift();for(const b of out.get(a)||[]){layer.set(b,Math.max(layer.get(b)??0,layer.get(a)+1));into.set(b,into.get(b)-1);if(!into.get(b))queue.push(b);}}
    const left=[...set].filter(a=>into.get(a)>0).sort();
    if(!left.length)break;
    into.set(left[0],0);layer.set(left[0],layer.get(left[0])??0);queue.push(left[0]);
  }
  layer.max=Math.max(0,...layer.values());
  const cols=new Map();
  for(const [a,c] of [...layer.entries()].sort((x,y)=>x[0].localeCompare(y[0]))){if(!cols.has(c))cols.set(c,[]);cols.get(c).push(a);}
  layer.pos=new Map();
  for(const [c,list] of cols)list.forEach((a,i)=>layer.pos.set(a,[(c-layer.max/2)*LAYER_GAP,(i-(list.length-1)/2)*ROW_GAP+(c%2?ROW_GAP:-ROW_GAP)/7]));
  return layer;
}
// The flow of `seeds`: their direct children (up to maxL) and up to `maxHops` hops of
// depends_on. An Inbox node joins the flow only as a seed.
function traceDeps(seeds,edges,points,maxL,maxHops,nodes){
  const inbox=new Set(inboxKinds());
  const down=new Map(),up=new Map(),kids=new Map();
  const add=(map,key,value)=>{if(!map.has(key))map.set(key,[]);map.get(key).push(value);};
  for(const e of edges){
    if(e.depends){add(down,e.from,e.to);add(up,e.to,e.from);}else add(kids,e.from,e.to);
  }
  const start=new Set(seeds);
  for(const s of seeds)for(const c of kids.get(s)||[])if((points.get(c)?.level??0)<=maxL)start.add(c);
  const limit=start.size>3?1:maxHops;
  const hops=new Map([...start].map(a=>[a,0]));
  for(const next of [down,up]){
    const queue=[...start];
    while(queue.length){
      const a=queue.shift(),d=hops.get(a);
      if(d>=limit)continue;
      for(const o of next.get(a)||[]){
        if(inbox.has(nodes.get(o)?.kind)&&!seeds.has(o))continue;
        if((points.get(o)?.level??0)>maxL)continue;
        if(!hops.has(o)||hops.get(o)>d+1){hops.set(o,d+1);queue.push(o);}
      }
    }
  }
  return hops;
}
function light(ui){
  const {graph,nodes,found,status}=ui;
  const maxL=nodes.size>LARGE?ui.maxLevel:ALL_LEVELS;
  const rawActive=found?chain(found,graph.edges,null,false):null;
  const shallowActive=rawActive?new Set([...rawActive].filter(a=>(graph.points.get(a)?.level??0)<=maxL)):null;
  const click=ui.click&&(!rawActive||rawActive.has(ui.click))?ui.click:null;
  const hops=click?traceDeps(new Set([click]),graph.edges,graph.points,maxL,2,nodes):(shallowActive&&shallowActive.size<=PATH_MAX?traceDeps(shallowActive,graph.edges,graph.points,maxL,1,nodes):null);
  const traced=hops?new Set(hops.keys()):null;
  // Keep nodes up to maxL (plus any traced nodes or direct children of a clicked node), hiding orphan 0-link roots in large graphs.
  const active=new Set();
  for(const [address,p] of graph.points){
    if(traced?.has(address)){active.add(address);continue;}
    if(rawActive&&!rawActive.has(address))continue;
    if(p.level>maxL)continue;
    if(!rawActive&&nodes.size>LARGE&&p.level===0&&!(graph.neighbors.get(address)?.size))continue;
    active.add(address);
  }
  const key=[...active].sort().join(' ')+(traced?'#flow':'');
  if(ui.active!==key){ui.active=key;graph.run(active,traced&&traced.size<=PATH_MAX?flowLayers(traced,graph.edges):null);}
  const steps=traced?flowLayers(traced,graph.edges):null;
  const vis=traced||active,candidates=[];
  for(const [address,dot] of graph.dotOf){
    const hidden=!active.has(address),on=Boolean(traced?.has(address)),faint=Boolean(traced&&!on&&!hidden);
    const p=graph.points.get(address),h=hops?.get(address)??0;
    for(const element of [dot,graph.labelOf.get(address),graph.stepOf.get(address)]){
      element.classList.toggle('graph-hidden',hidden);element.classList.toggle('graph-lit',on);element.classList.toggle('graph-faint',faint);
      element.classList.toggle('graph-hop-1',on&&h===1);element.classList.toggle('graph-hop-2',on&&h===2);element.classList.toggle('graph-hop-3',on&&h>=3);
    }
    graph.grow(address,on&&Boolean(click));
    graph.labelOf.get(address).classList.remove('graph-label-shown');
    if(!hidden&&(!traced||on)){p.forceLabel=Boolean(on&&traced.size<=PATH_MAX);p.priority=(address===click?0:on?1:2)*10+p.level;candidates.push(p);}
    const step=graph.stepOf.get(address);step.textContent=on?String(steps.get(address)+1):'';step.classList.toggle('graph-step-shown',on);
  }
  graph.candidates=candidates.sort((a,b)=>a.priority-b.priority||a.address.localeCompare(b.address));
  for(const e of graph.edges){
    const hidden=!(active.has(e.from)&&active.has(e.to)),on=Boolean(traced?.has(e.from)&&traced.has(e.to));
    const h=on?Math.max(hops.get(e.from)||0,hops.get(e.to)||0):0,fromL=graph.points.get(e.from)?.level||0;
    e.element.classList.toggle('graph-hidden',hidden);e.element.classList.toggle('graph-lit',on);e.element.classList.toggle('graph-faint',Boolean(traced&&!on&&!hidden));
    e.element.classList.toggle('graph-hop-1',on&&h<=1);e.element.classList.toggle('graph-hop-2',on&&h===2);e.element.classList.toggle('graph-hop-3',on&&h>=3);
    e.element.classList.toggle('graph-edge-l0',!on&&!e.depends&&fromL===0);e.element.classList.toggle('graph-edge-l1',!on&&!e.depends&&fromL>=1);
  }
  graph.focus(vis&&vis.size?vis:null);
  graph.cull();
  const arrows=graph.edges.filter(e=>e.depends&&active.has(e.from)&&active.has(e.to)).length;
  status.textContent=traced?(traced.size?traced.size+' nodes in flow ('+(steps.max+1)+' steps) · click background to reset':'No node matches these filters.'):active.size+' of '+nodes.size+' nodes shown ('+arrows+' flow links) · click any node to trace its flow';
  showTrail(ui,traced,steps,click);
  if(ui.card.dataset.address!==(click||''))nodeCard(ui.card,graph,nodes,click,ui.pick,ui.focusFlow);
  if(!click)ui.card.dataset.address='';
}
// The steps of the path in flow order. Each step picks its node, and Open opens the picked node.
function showTrail(ui,lit,steps,click){
  const {trail,nodes}=ui;trail.replaceChildren();trail.hidden=!lit||!lit.size;
  if(trail.hidden)return;
  const order=[...lit].sort((a,b)=>steps.get(a)-steps.get(b)||(nodes.get(a)?.title||a).localeCompare(nodes.get(b)?.title||b));
  order.slice(0,TRAIL).forEach((address,i)=>{
    if(i)trail.append(E('span','→','graph-trail-arrow'));
    const node=nodes.get(address),hop=E('button',undefined,'graph-hop');hop.type='button';hop.dataset.address=address;
    hop.append(E('span',String(steps.get(address)+1),'graph-hop-step'),E('span',undefined,'status-circle '+(node?.signal?.tone||'grey')),node?.title||address);
    if(address===click)hop.setAttribute('aria-current','step');
    hop.onclick=()=>ui.pick(address);trail.append(hop);
  });
  if(order.length>TRAIL)trail.append(E('span','+'+(order.length-TRAIL)+' more','hint'));
}
