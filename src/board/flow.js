// Flow view of the board (view=flow): the nodes of one relation from left to right.
// - The picker Relation lists the registry in key order. Without relation=<key> in the link,
//   the first view is the first key that is not a system key (depends_on when there is none).
//   A pick writes relation=<key> into the link. An unknown key shows the first view.
// - The board reads the members of the relation (in_relation) in pages of 25 and draws them
//   (flow-draw.js). A depends_on arrow goes from the blocker to the node that waits. The arrow
//   of every other relation goes from the node to its target.
// - Then the board reads the facts of the steps (flow-facts.js). The dots and the card show
//   after every read ends. A dot takes the worst tone of its step and the facts.
// - A click lights the node, its neighbours and their edges, and fades the rest. The card
//   (flow-card.js) shows the node. A transfer badge opens the other relation at the node.
// - A filter or a search fades the nodes that do not match. It fades no edge.
// - The status line shows only the load, an error, the no-match text and the empty text.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectPart,PAGE} from 'board/api.js';
import {commonObjectFilters,matches} from 'board/filters.js';
import {relationRegistry,relationTitles} from 'board/relations.js';
import {drawFlow} from 'board/flow-draw.js';
import {flowCard} from 'board/flow-card.js';
import {factsOf,readFacts,rolledTone,hiddenFacts} from 'board/flow-facts.js';
const FIELDS='title,signal,relations,item_id';
// The shown page. A new board render closes it first.
let shown=null;
// Free the camera of the shown drawing before its canvas leaves the page.
export function closeFlowPage(){shown?.flow?.camera.destroy();shown=null;}
export async function renderFlowPage(root,generation){
  document.title='Flow · Joppa';
  const page=E('section',undefined,'flow-page');
  const live=()=>generation===state.loadNumber&&page.isConnected;
  const status=E('p',undefined,'flow-status hint');status.setAttribute('role','status');
  const description=E('p',undefined,'flow-description hint');
  const view=E('div',undefined,'flow-view'),card=E('aside',undefined,'flow-card');card.setAttribute('aria-label','Picked node');
  const say=text=>{status.textContent=text;status.hidden=!text;};
  const ui={key:'',flow:null,nodes:new Map(),facts:new Map(),tones:new Map(),found:null,click:null,load:0,card,say};
  ui.pick=address=>{ui.click=address;paint(ui);};
  closeFlowPage();shown=ui;
  const [registry,titles]=await Promise.all([relationRegistry(),relationTitles()]);
  if(generation!==state.loadNumber)return;
  const filters=commonObjectFilters(root,E('div'),async({params,text,scope,filtering,current})=>{
    const next=filtering?await matchSet(text,params,scope,current):null;
    if(next===undefined||!current())return;
    ui.found=next;paint(ui);
  });
  const keys=Object.keys(registry).sort((a,b)=>a<b?-1:a>b?1:0);
  const picker=E('select',undefined,'flow-relation');picker.name='relation';picker.setAttribute('aria-label','Relation');
  for(const key of keys){const option=E('option',titles[key]||key);option.value=key;picker.append(option);}
  filters.controls.prepend(picker);
  const linked=new URLSearchParams(location.search).get('relation');
  picker.value=keys.includes(linked)?linked:keys.find(key=>!registry[key].system)||'depends_on';
  // A pick of a relation writes it into the link and keeps the other query values.
  const choose=(key,address)=>{
    picker.value=key;
    const next=new URLSearchParams(location.search);next.set('relation',key);
    history.replaceState(history.state,'',location.pathname+'?'+next+location.hash);
    show(key,address);
  };
  picker.onchange=()=>choose(picker.value,null);
  ui.transfer=(key,address)=>choose(key,address);
  const show=async(key,address)=>{
    const mine=++ui.load,current=()=>live()&&mine===ui.load;
    description.textContent=registry[key]?.description||'';description.hidden=!description.textContent;
    ui.flow?.camera.destroy();ui.flow?.canvas.remove();ui.flow=null;ui.click=null;flowCard(card,{});say('Loading nodes…');
    const nodes=new Map();let cursor,facts;
    try{
      do{
        const part=await objectPart({in_relation:key,fields:FIELDS,limit:PAGE,...(cursor?{cursor}:{})});
        if(!current())return;
        for(const item of part.items)nodes.set(item.address,item);
        say('Loading nodes… '+nodes.size+' of '+part.total);cursor=part.next_cursor;
      }while(cursor);
      facts=await readFacts(nodes,key,current,toneOf);if(!facts)return;
    }catch(e){if(current())say(e.message);return;}
    ui.key=key;ui.nodes=nodes;ui.facts=facts;
    if(!nodes.size)return say('No node holds this relation.');
    const edges=flowEdges(nodes,key),hidden=hiddenFacts(nodes,key,edges);
    ui.tones=new Map([...nodes.values()].map(node=>[node.address,rolledTone(toneOf(node),factsOf(node,key).map(a=>facts.get(a)?.tone))]));
    const items=[...nodes.values()].filter(node=>!hidden.has(node.address)).map(node=>({address:node.address,tone:ui.tones.get(node.address),
      label:labelOf(node,key),color:node.relations?.[key]?.color,badges:badges(node,key,registry,titles)}));
    ui.flow=drawFlow(view,items,edges,{pick:ui.pick,transfer:ui.transfer});
    ui.click=address&&ui.flow.parts.has(address)?address:null;
    paint(ui);
  };
  view.append(card);page.append(description,status,view);root.append(page);
  await show(picker.value,null);
}
// A node without a tone takes the tone blue.
function toneOf(node){return node?.signal?.tone||'blue';}
// A node shows its own title in the relation, else its object title.
function labelOf(node,key){return node.relations?.[key]?.title||node.title||node.address;}
// The other relations of the node that are not system relations, in key order.
function badges(node,key,registry,titles){
  return Object.keys(node.relations||{}).filter(k=>k!==key&&registry[k]&&!registry[k].system).sort((a,b)=>a<b?-1:a>b?1:0).map(k=>({key:k,title:titles[k]||k}));
}
// The edges of the relation between two members, in the stored order of each node. A
// depends_on edge points from its target, the blocker, to the node that waits.
function flowEdges(nodes,key){
  const list=[];
  for(const node of nodes.values())for(const edge of node.relations?.[key]?.edges||[]){
    if(!nodes.has(edge.to))continue;
    const [from,to]=key==='depends_on'?[edge.to,node.address]:[node.address,edge.to];
    list.push({from,to,when:edge.when||'',color:edge.color||null});
  }
  return list;
}
// Mark the picked node, its neighbours and their edges as lit and the rest as faint. A node
// that does not match the filters is faint too.
function paint(ui){
  const {flow,found,click,nodes}=ui;if(!flow)return;
  const near=click?new Set([click]):null;
  if(click)for(const e of flow.edges)if(e.from===click||e.to===click){near.add(e.from);near.add(e.to);}
  let matching=0;
  for(const [address,parts] of flow.parts){
    const lit=Boolean(near?.has(address)),match=!found||found.has(address);if(match)matching++;
    for(const element of parts){element.classList.toggle('flow-lit',lit);element.classList.toggle('flow-faint',Boolean(near&&!lit)||!match);}
  }
  for(const e of flow.edges){
    const lit=Boolean(click&&(e.from===click||e.to===click));
    for(const element of [e.element,e.label])if(element){element.classList.toggle('flow-lit',lit);element.classList.toggle('flow-faint',Boolean(click&&!lit));}
  }
  ui.say(matching?'':'No node matches these filters.');
  // The card head and the steps show the dot tone, Status the own tone of the node.
  const node=nodes.get(click),facts=factsOf(node,ui.key).map(a=>({address:a,...ui.facts.get(a)}));
  flowCard(ui.card,click?{address:click,node,label:labelOf(node,ui.key),edges:flow.edges,facts,pick:ui.pick,
    nameOf:a=>labelOf(nodes.get(a)||{address:a},ui.key),toneOf:a=>ui.tones.get(a)||toneOf(nodes.get(a)),ownTone:toneOf(node)}:{});
}
// The addresses that match the filters: the node list for the filters, the search for a text.
// Both read every page of 25.
async function matchSet(text,params,scope,current){
  const clean={...params};delete clean.depth;
  const list=text?{...clean,q:text,limit:PAGE,...scope}:{parent:'*',...clean,limit:PAGE,fields:'kind'};
  const found=new Set();let cursor;
  do{
    const part=await matches(text,cursor?{...list,cursor}:list);if(!current())return undefined;
    for(const item of part.items){
      if(!text)found.add(item.address);
      else if(item.workspace===state.ws&&!item.historical)found.add(item.kind+':'+item.id);
    }
    cursor=part.next_cursor;
  }while(cursor);
  return found;
}
