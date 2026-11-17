// Flow view of the board (view=flow): the nodes of one flow from left to right. A flow is a
// relation of the registry that is not a system relation. Flow view shows no depends_on, no
// related, no filter and no search.
// - The header holds two pickers. Flow group lists All groups, the group names in alphabetical
//   order, and Ungrouped (-) when grouped and ungrouped flows both exist. It shows only when a
//   flow has a group. Relation lists the flows of the picked group in key order.
// - group=<name> and relation=<key> in the link pick them. An unknown group shows All groups. A
//   relation outside the group shows the first flow of the group. The first load changes no
//   link value. A group pick writes group, and when the group does not hold the shown flow, it
//   shows the first flow of the group. A relation pick writes relation and removes node.
// - node=<address> in the link keeps the picked node. The first load reads it, an address or
//   the public item ID of a drawn node, and changes no link value. A pick writes node, × and a
//   clear remove it. An unknown node picks nothing and stays in the link.
// - A linked node in a wide flow opens near the node (focus). A small flow opens whole.
// - The board reads the members of the relation (in_relation) in pages of 25 and draws them
//   (flow-draw.js). The arrow goes from the node to its target.
// - Then the board reads the facts of the steps (flow-facts.js). The dots and the card show
//   after every read ends. A dot takes the worst tone of its step and the facts.
// - A click lights the node, its neighbours and their edges, and fades the rest. The card
//   (flow-card.js) shows the node. A transfer badge opens the other flow at the node and writes
//   relation and node into the link. A flow outside the picked group picks All groups.
// - The status line shows only the load, an error and the empty texts.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {objectPart,PAGE} from 'board/api.js';
import {relationRegistry,relationTitles} from 'board/relations.js';
import {drawFlow,FIT_MARGIN} from 'board/flow-draw.js';
import {flowCard} from 'board/flow-card.js';
import {factsOf,readFacts,rolledTone,hiddenFacts} from 'board/flow-facts.js';
const FIELDS='title,signal,relations,item_id';
// The value of Ungrouped in Flow group. The server refuses it as a group name.
const UNGROUPED='-';
const byKey=(a,b)=>a<b?-1:a>b?1:0;
// The shown page. A new board render closes it first.
let shown=null;
// Free the camera of the shown drawing before its canvas leaves the page.
export function closeFlowPage(){shown?.flow?.camera.destroy();shown=null;}
export async function renderFlowPage(root,generation){
  document.title='Flow · Joppa';
  const page=E('section',undefined,'flow-page');
  const live=()=>generation===state.loadNumber&&page.isConnected;
  const status=E('p',undefined,'flow-status hint');status.setAttribute('role','status');
  const description=E('p',undefined,'flow-description hint');description.hidden=true;
  const view=E('div',undefined,'flow-view'),card=E('aside',undefined,'flow-card');card.setAttribute('aria-label','Picked node');
  const say=text=>{status.textContent=text;status.hidden=!text;};
  // collapsed: the card shows only its head. It stays for the next pick.
  const ui={key:'',flow:null,nodes:new Map(),facts:new Map(),tones:new Map(),click:null,collapsed:false,load:0,card};
  ui.pick=address=>{ui.click=address;relink({node:address});paint(ui);};
  closeFlowPage();shown=ui;
  const [registry,titles]=await Promise.all([relationRegistry(),relationTitles()]);
  if(generation!==state.loadNumber)return;
  const keys=Object.keys(registry).filter(key=>!registry[key].system).sort(byKey);
  page.append(description,status);
  if(!keys.length){say('No flows in this workspace.');root.append(page);return;}
  const groupOf=key=>registry[key].group||'';
  const named=[...new Set(keys.map(groupOf).filter(Boolean))].sort((a,b)=>a.localeCompare(b));
  const inGroup=(key,group)=>!group||(group===UNGROUPED?!groupOf(key):groupOf(key)===group);
  const controls=E('div',undefined,'filter-fields');
  const select=(name,label,options)=>{
    const element=E('select');element.name=name;element.setAttribute('aria-label',label);
    for(const [value,title] of options){const option=E('option',title);option.value=value;element.append(option);}
    return element;
  };
  const groups=[['','All groups'],...named.map(name=>[name,name]),...(keys.some(key=>!groupOf(key))?[[UNGROUPED,'Ungrouped']]:[])];
  const grouper=select('group','Flow group',groups);
  const picker=select('relation','Relation',[]);picker.classList.add('flow-relation');
  // The flows of the group, in key order.
  const fill=group=>{
    picker.replaceChildren(...keys.filter(key=>inGroup(key,group)).map(key=>{const option=E('option',titles[key]||key);option.value=key;return option;}));
    return [...picker.options].map(option=>option.value);
  };
  if(named.length)controls.append(grouper);
  controls.append(picker);
  const linked=new URLSearchParams(location.search);
  grouper.value=named.length&&groups.some(([value])=>value===linked.get('group'))?linked.get('group'):'';
  const listed=fill(grouper.value);
  picker.value=listed.includes(linked.get('relation'))?linked.get('relation'):listed[0];
  // A pick of a relation writes it and the node into the link and keeps the other query values.
  const choose=(key,address)=>{
    picker.value=key;relink({relation:key,node:address});
    show(key,address);
  };
  picker.onchange=()=>choose(picker.value,null);
  grouper.onchange=()=>{
    const shownKey=picker.value,list=fill(grouper.value);relink({group:grouper.value});
    if(list.includes(shownKey))picker.value=shownKey;else choose(list[0],null);
  };
  ui.transfer=(key,address)=>{
    if(!inGroup(key,grouper.value)){grouper.value='';fill('');relink({group:null});}
    choose(key,address);
  };
  // `first`: the first load, which reads node= of the link and can focus the node.
  const show=async(key,address,first=false)=>{
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
    ui.flow=drawFlow(view,items,edges,{pick:ui.pick,transfer:ui.transfer});say('');
    // The node is an address, or the public item ID of a node. Only a drawn node is picked.
    const target=address&&(nodes.has(address)?address:[...nodes.values()].find(node=>node.item_id===address)?.address);
    ui.click=target&&ui.flow.parts.has(target)?target:null;
    if(first&&ui.click)focus(ui.flow,ui.click);
    paint(ui);
  };
  view.append(card);page.append(view);root.append(controls,page);
  await show(picker.value,new URLSearchParams(location.search).get('node'),true);
}
// Write the link values of `changes` and keep the others. A null value removes its key.
function relink(changes){
  const next=new URLSearchParams(location.search);
  for(const [key,value] of Object.entries(changes))if(value)next.set(key,value);else next.delete(key);
  history.replaceState(history.state,'',location.pathname+'?'+next+location.hash);
}
// When the whole flow needs more than 1 world unit for one pixel, the view fits the node, its
// neighbours and the waypoints and arc middles of their edges, with at least 1 world unit for
// one pixel. Else the whole view stays.
function focus({camera,position,edges},address){
  if(camera.unit()<=1)return;
  const points=[position.get(address)];
  for(const e of edges)if(e.from===address||e.to===address)points.push(position.get(e.from),position.get(e.to),...(e.via||[]),...(e.back?[e.mid]:[]));
  camera.fit(points,FIT_MARGIN,1);
}
// A node without a tone takes the tone blue.
function toneOf(node){return node?.signal?.tone||'blue';}
// A node shows its own title in the relation, else its object title.
function labelOf(node,key){return node.relations?.[key]?.title||node.title||node.address;}
// The other relations of the node that are not system relations, in key order.
function badges(node,key,registry,titles){
  return Object.keys(node.relations||{}).filter(k=>k!==key&&registry[k]&&!registry[k].system).sort(byKey).map(k=>({key:k,title:titles[k]||k}));
}
// The edges of the relation between two members, in the stored order of each node.
function flowEdges(nodes,key){
  const list=[];
  for(const node of nodes.values())for(const edge of node.relations?.[key]?.edges||[]){
    if(!nodes.has(edge.to))continue;
    list.push({from:node.address,to:edge.to,when:edge.when||'',color:edge.color||null});
  }
  return list;
}
// Mark the picked node, its neighbours and their edges as lit and the rest as faint.
function paint(ui){
  const {flow,click,nodes}=ui;if(!flow)return;
  const near=click?new Set([click]):null;
  if(click)for(const e of flow.edges)if(e.from===click||e.to===click){near.add(e.from);near.add(e.to);}
  for(const [address,parts] of flow.parts){
    const lit=Boolean(near?.has(address));
    for(const element of parts){element.classList.toggle('flow-lit',lit);element.classList.toggle('flow-faint',Boolean(near&&!lit));}
  }
  for(const e of flow.edges){
    const lit=Boolean(click&&(e.from===click||e.to===click));
    for(const element of [e.element,e.label])if(element){element.classList.toggle('flow-lit',lit);element.classList.toggle('flow-faint',Boolean(click&&!lit));}
  }
  // The card head and the steps show the dot tone, Status the own tone of the node.
  const node=nodes.get(click),facts=factsOf(node,ui.key).map(a=>({address:a,...ui.facts.get(a)}));
  flowCard(ui.card,click?{address:click,node,label:labelOf(node,ui.key),relation:ui.key,edges:flow.edges,facts,pick:ui.pick,
    nameOf:a=>labelOf(nodes.get(a)||{address:a},ui.key),toneOf:a=>ui.tones.get(a)||toneOf(nodes.get(a)),ownTone:toneOf(node),
    collapsed:ui.collapsed,collapse:on=>{ui.collapsed=on;}}:{});
}
