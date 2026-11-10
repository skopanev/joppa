// The facts of the steps of Flow view: relations.<key>.facts of each member, in stored order.
// - The member read gives the facts that are members. The board reads every other fact with
//   the address read, up to 25 addresses in one request (title, signal, item_id, present).
// - A fact without a tone takes blue, and an archived fact takes purple.
// - The dot of a step takes the worst tone of the step and its facts: red, orange, yellow,
//   blue, green. A purple fact does not count.
// - A member without an edge in the drawing that is a fact of a drawn step has no dot.
import {objectPart,PAGE} from 'board/api.js';
const WORST=['red','orange','yellow','blue','green'];
// The facts of `node` in the relation `key`.
export function factsOf(node,key){return node?.relations?.[key]?.facts||[];}
// Read the facts that are not members: a Map of address to {title, tone, item_id}, with the
// members too. `current()` false stops the read with undefined.
export async function readFacts(nodes,key,current,toneOf){
  const info=new Map([...nodes.values()].map(n=>[n.address,{title:n.title||n.address,tone:toneOf(n),item_id:n.item_id}]));
  const missing=[...new Set([...nodes.values()].flatMap(n=>factsOf(n,key)))].filter(a=>!info.has(a));
  const size=Number(PAGE);
  for(let i=0;i<missing.length;i+=size){
    const batch=missing.slice(i,i+size);
    const part=await objectPart({address:batch.join(','),fields:'title,signal,item_id,present'});
    if(!current())return undefined;
    // One address gives the object itself, more give items in the order of the request.
    const items=part.items||[{title:part.document?.data?.title,signal:part.signal,item_id:part.item_id,present:part.document?.present!==false}];
    batch.forEach((address,n)=>{
      const item=items[n]||{};
      info.set(address,{title:item.title||address,tone:item.present===false?'purple':item.signal?.tone||'blue',item_id:item.item_id});
    });
  }
  return info;
}
// The worst tone of `own` and the fact tones. A tone outside the order counts for nothing.
export function rolledTone(own,tones){
  const ranks=[own,...tones].map(t=>WORST.indexOf(t)).filter(r=>r>=0);
  return ranks.length?WORST[Math.min(...ranks)]:own;
}
// The members with no drawn edge that a drawn step lists as a fact. The steps with edges come
// first, then the other drawn members in address order.
export function hiddenFacts(nodes,key,edges){
  const linked=new Set(edges.flatMap(e=>[e.from,e.to]));
  const hidden=new Set();
  const hide=node=>{for(const fact of factsOf(node,key))if(fact!==node.address&&nodes.has(fact)&&!linked.has(fact))hidden.add(fact);};
  for(const node of nodes.values())if(linked.has(node.address))hide(node);
  for(const node of nodes.values())if(!linked.has(node.address)&&!hidden.has(node.address))hide(node);
  return hidden;
}
