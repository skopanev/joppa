// The layout of Flow view: columns from left to right, and the back edges of each cycle.
// - Take the nodes in address order. Walk depth first from each node without incoming edges,
//   then from each node that is not visited yet. Take the edges of a node in their stored order.
// - An edge to a node on the current walk stack is a back edge. It gets `back: true`.
// - The column of a node is the length of the longest path to it without back edges.
// - The nodes of one column go down in address order, round the middle line.
export const COLUMN_GAP=280,ROW_GAP=140;
// A node label has at most LINES lines of at most LINE characters.
export const LINE=18,LINES=3;
// The lines of a node label:
// - Split at white space. A word of more than LINE characters keeps LINE-1 and gets "…".
// - A word joins the line when the line, a space and the word fit in LINE characters, else
//   it starts the next line. An empty line takes the next word.
// - When words remain after the last line, that line drops end words until it and "…" fit.
//   With no word left, its first LINE-1 characters stay. The line ends with "…".
export function wrapLabel(text){
  const size=s=>[...s].length,cut=s=>[...s].slice(0,LINE-1).join('')+'…';
  let rest=text.split(/\s+/).filter(Boolean).map(w=>size(w)>LINE?cut(w):w);
  const lines=[];
  while(rest.length&&lines.length<LINES){
    let line=rest[0],i=1;
    while(i<rest.length&&size(line)+1+size(rest[i])<=LINE)line+=' '+rest[i++];
    lines.push(rest.slice(0,i));rest=rest.slice(i);
  }
  const rows=lines.map(words=>words.join(' '));
  if(rest.length){
    let words=lines[LINES-1];
    while(words.length&&size(words.join(' '))+1>LINE)words=words.slice(0,-1);
    rows[LINES-1]=words.length?words.join(' ')+'…':cut(rows[LINES-1]);
  }
  return rows;
}
// `addresses` come in address order. Each edge is {from, to} in its stored order.
export function flowLayout(addresses,edges){
  const out=new Map(addresses.map(a=>[a,[]])),into=new Map(addresses.map(a=>[a,0]));
  for(const e of edges){out.get(e.from).push(e);into.set(e.to,into.get(e.to)+1);}
  // seen: 1 while the node is on the walk stack, 2 after its walk ends.
  const seen=new Map(),done=[];
  const walk=start=>{
    const stack=[[start,0]];seen.set(start,1);
    while(stack.length){
      const top=stack[stack.length-1],list=out.get(top[0]);
      if(top[1]===list.length){seen.set(top[0],2);done.push(top[0]);stack.pop();continue;}
      const e=list[top[1]++],mark=seen.get(e.to);
      e.back=mark===1;
      if(!mark){seen.set(e.to,1);stack.push([e.to,0]);}
    }
  };
  for(const a of addresses)if(!into.get(a)&&!seen.has(a))walk(a);
  for(const a of addresses)if(!seen.has(a))walk(a);
  // The reverse of the walk end order is a topological order of the edges without back edges.
  const column=new Map(addresses.map(a=>[a,0]));
  for(const a of done.reverse())for(const e of out.get(a))if(!e.back)column.set(e.to,Math.max(column.get(e.to),column.get(a)+1));
  const columns=new Map();
  for(const a of addresses){const c=column.get(a);if(!columns.has(c))columns.set(c,[]);columns.get(c).push(a);}
  const position=new Map();
  for(const [c,list] of columns)list.forEach((a,i)=>position.set(a,{address:a,x:c*COLUMN_GAP,y:(i-(list.length-1)/2)*ROW_GAP}));
  return {column,position};
}
