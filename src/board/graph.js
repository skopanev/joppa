// The graph view of the board (view=graph): every present node with its parent edges and its
// depends_on arrows, in a live force layout. There are two modes:
// - A filter, a search or a field choice is on: only the matches and their path stay. The
//   other nodes and edges are hidden, and the layout runs on the path alone in flow layers.
// - No filter: every node shows, muted. A click on a dot lights its path, and the other
//   nodes fade almost out.
// The path of a node set is the set, every node below it, every node that they depend on and
// every node that depends on them at any distance, and the parents of all these up to the
// root. Each lit node shows its step number, and the trail above the canvas lists the steps.
// A picked node also shows its card over the canvas (graph-card.js).
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectPart} from 'board/api.js';
import {inboxKinds} from 'board/object-types.js';
import {commonObjectFilters,matches} from 'board/filters.js';
import {drawGraph} from 'board/graph-layout.js';
import {nodeCard} from 'board/graph-card.js';
// The graph reads lean pages of the server maximum: a path needs every node.
const NODE_PAGE='100',SEARCH_PAGE='50',FIELDS='kind,parent,title,signal,links';
// The graph names every lit node while this many nodes or fewer are lit, and the trail lists
// this many steps at most.
const LABELS=80,TRAIL=60;
export async function renderGraphPage(root,generation){
  document.title='Graph · Joppa';
  const page=E('section',undefined,'graph-page');
  const live=()=>generation===state.loadNumber&&page.isConnected;
  const status=E('p','Loading nodes…','graph-status hint');status.setAttribute('role','status');
  const trail=E('nav',undefined,'graph-picked');trail.setAttribute('aria-label','Path steps');trail.hidden=true;
  const view=E('div',undefined,'graph-view'),card=E('aside',undefined,'graph-card');card.hidden=true;card.setAttribute('aria-label','Picked node');
  const ui={graph:null,nodes:null,found:null,click:null,active:'',status,trail,card};
  const paint=()=>{if(ui.graph)light(ui);};
  // A click on the picked dot again, on the canvas or on × clears the pick.
  ui.pick=address=>{ui.click=address===ui.click?null:address;paint();};
  const filters=commonObjectFilters(root,E('div'),async({params,text,scope,filtering,current})=>{
    const next=filtering?await matchSet(text,params,scope,current):null;
    if(next===undefined||!current())return;
    ui.found=next;ui.click=null;paint();
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
  // The root picker offers each root node of a type that is not an Inbox.
  const inbox=new Set(inboxKinds()),roots=[...nodes.values()].filter(n=>!n.parent&&!inbox.has(n.kind)).sort((a,b)=>(a.title||'').localeCompare(b.title||''));
  filters.offer('parent','Object root',[['','All roots'],...roots.map(n=>[n.address,n.title||n.address])]);
  view.append(card);ui.nodes=nodes;ui.graph=drawGraph(nodes,ui.pick,view);paint();
}
// The addresses that match the filters: every page of the node list, or the first page of the
// ranked search. Undefined when a newer filter replaced this one.
async function matchSet(text,params,scope,current){
  const list=text?{...params,q:text,limit:SEARCH_PAGE,...scope}:{parent:'*',...params,limit:NODE_PAGE,fields:'kind'};
  const found=new Set();let cursor;
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
// The path of `seeds` (see the top of this file), inside `within` when it is given.
export function chain(seeds,edges,within){
  const down=new Map(),up=new Map(),kids=new Map(),parent=new Map();
  const add=(map,key,value)=>{if(!map.has(key))map.set(key,[]);map.get(key).push(value);};
  for(const e of edges){
    if(within&&!(within.has(e.from)&&within.has(e.to)))continue;
    if(e.depends){add(down,e.from,e.to);add(up,e.to,e.from);}else{add(kids,e.from,e.to);parent.set(e.to,e.from);}
  }
  const reach=(start,next)=>{const seen=new Set(start),queue=[...start];while(queue.length)for(const o of next.get(queue.pop())||[])if(!seen.has(o)){seen.add(o);queue.push(o);}return seen;};
  const branch=reach(seeds,kids),lit=new Set([...reach(branch,down),...reach(branch,up)]);
  for(const address of [...lit])for(let p=parent.get(address);p&&!lit.has(p);p=parent.get(p))lit.add(p);
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
  return layer;
}
function light(ui){
  const {graph,nodes,found,status}=ui;
  // A filter keeps only its path. A click inside it lights the path of the clicked node.
  const active=found?chain(found,graph.edges):null;
  const click=ui.click&&(!active||active.has(ui.click))?ui.click:null;
  const lit=click?chain(new Set([click]),graph.edges,active):active;
  const key=active?[...active].sort().join(' '):'';
  if(ui.active!==key){ui.active=key;graph.run(active,active&&flowLayers(active,graph.edges));}
  const steps=lit?flowLayers(lit,graph.edges):null;
  const named=lit&&lit.size<=LABELS?lit:null;
  for(const [address,dot] of graph.dotOf){
    const hidden=Boolean(active&&!active.has(address)),on=Boolean(lit?.has(address));
    for(const element of [dot,graph.labelOf.get(address),graph.stepOf.get(address)]){
      element.classList.toggle('graph-hidden',hidden);element.classList.toggle('graph-lit',on);element.classList.toggle('graph-faint',Boolean(lit&&!on&&!hidden));
    }
    graph.grow(address,on&&Boolean(click));
    graph.labelOf.get(address).classList.toggle('graph-label-shown',named?named.has(address):!nodes.get(address)?.parent);
    const step=graph.stepOf.get(address);step.textContent=on?String(steps.get(address)+1):'';step.classList.toggle('graph-step-shown',on);
  }
  for(const e of graph.edges){
    const hidden=Boolean(active&&!(active.has(e.from)&&active.has(e.to))),on=Boolean(lit?.has(e.from)&&lit.has(e.to));
    e.element.classList.toggle('graph-hidden',hidden);e.element.classList.toggle('graph-lit',on);e.element.classList.toggle('graph-faint',Boolean(lit&&!on&&!hidden));
  }
  graph.focus(lit&&lit.size?lit:null);
  const arrows=graph.edges.filter(e=>e.depends).length;
  status.textContent=lit?(lit.size?lit.size+' of '+nodes.size+' nodes on the path in '+(steps.max+1)+' steps':'No node matches these filters.'):nodes.size+' nodes · '+arrows+' depends_on links';
  showTrail(ui,lit,steps,click);
  if(ui.card.dataset.address!==(click||''))nodeCard(ui.card,graph,nodes,click,ui.pick);
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
