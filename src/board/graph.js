// The graph view of the board (view=graph). Every present node is a dot in the colour of its
// status tone, with its parent edges and its directed depends_on arrows. The filter bar of the
// board and a click on a dot light up the nodes and their full depends_on chain, upstream and
// downstream. Every other node and edge dims.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectPart} from 'board/api.js';
import {objectURL} from 'board/links.js';
import {commonObjectFilters,matches} from 'board/filters.js';
import {radialLayout,drawGraph} from 'board/graph-layout.js';
// The graph reads lean pages of the server maximum: a chain needs every node.
const NODE_PAGE='100',SEARCH_PAGE='50',FIELDS='kind,parent,title,signal,links';
// The graph names labels only while this many nodes or fewer are lit.
const LABELS=60;
export async function renderGraphPage(root,generation){
  document.title='Graph · Joppa';
  const page=E('section',undefined,'graph-page');
  const live=()=>generation===state.loadNumber&&page.isConnected;
  const status=E('p','Loading nodes…','graph-status hint');status.setAttribute('role','status');
  const picked=E('div',undefined,'graph-picked');picked.hidden=true;
  const view=E('div',undefined,'graph-view');
  let graph=null,nodes=null,click=null,found=null;
  const paint=()=>{if(graph)light(graph,nodes,click?new Set([click]):found,status,picked);};
  commonObjectFilters(root,E('div'),async({params,text,scope,filtering,current})=>{
    const next=filtering?await matchSet(text,params,scope,current):null;
    if(next===undefined||!current())return;
    found=next;paint();
  });
  page.append(status,picked,view);root.append(page);
  nodes=new Map();let cursor;
  try{
    do{
      const part=await objectPart({parent:'*',limit:NODE_PAGE,fields:FIELDS,...(cursor?{cursor}:{})});
      if(!live())return;
      for(const item of part.items)nodes.set(item.address,item);
      status.textContent='Loading nodes… '+nodes.size+' of '+part.total;cursor=part.next_cursor;
    }while(cursor);
  }catch(e){if(live())status.textContent=e.message;return;}
  if(!nodes.size){status.textContent='No objects here yet.';return;}
  graph=drawGraph(nodes,radialLayout(nodes),address=>{click=address===click?null:address;paint();});
  view.append(graph.canvas);paint();
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
// The seeds and every node that a seed depends on, directly or through other nodes, and every
// node that depends on a seed in the same way.
export function chain(seeds,edges){
  const out=new Map(),into=new Map();
  for(const edge of edges)if(edge.depends){
    if(!out.has(edge.from))out.set(edge.from,[]);out.get(edge.from).push(edge.to);
    if(!into.has(edge.to))into.set(edge.to,[]);into.get(edge.to).push(edge.from);
  }
  const lit=new Set(seeds);
  for(const next of [out,into]){
    const queue=[...seeds],seen=new Set(seeds);
    while(queue.length)for(const other of next.get(queue.pop())||[])if(!seen.has(other)){seen.add(other);lit.add(other);queue.push(other);}
  }
  return lit;
}
function light(graph,nodes,seeds,status,picked){
  const lit=seeds?chain(seeds,graph.edges):null;
  for(const [address,dot] of graph.dotOf){
    dot.classList.toggle('graph-dim',Boolean(lit&&!lit.has(address)));
    dot.classList.toggle('graph-seed',Boolean(seeds?.has(address)));
  }
  for(const edge of graph.edges)edge.element.classList.toggle('graph-dim',Boolean(lit&&!(lit.has(edge.from)&&lit.has(edge.to))));
  const named=lit&&lit.size<=LABELS?lit:null;
  for(const [address,label] of graph.labelOf){
    label.classList.toggle('graph-label-shown',named?named.has(address):nodes.get(address)?.parent==null);
    label.classList.toggle('graph-dim',Boolean(lit&&!lit.has(address)));
  }
  const arrows=graph.edges.filter(edge=>edge.depends).length;
  status.textContent=lit?lit.size+' of '+nodes.size+' nodes lit: '+(seeds.size)+' matched and their depends_on chain':nodes.size+' nodes · '+arrows+' depends_on links';
  // A picked dot names its node and links to its object page.
  const one=seeds?.size===1&&graph.dotOf.has([...seeds][0])?nodes.get([...seeds][0]):null;
  picked.hidden=!one;picked.replaceChildren();
  if(one){const open=E('a',one.title||one.address,'graph-open');open.href=objectURL(one.address);picked.append(E('span',undefined,'status-circle '+(one.signal?.tone||'grey')),open);}
}
