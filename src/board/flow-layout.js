// The layout of Flow view: columns from left to right, and the back edges of each cycle.
// - Take the nodes in address order. Walk depth first from each node without incoming edges,
//   then from each node that is not visited yet. Take the edges of a node in their stored order.
// - An edge to a node on the current walk stack is a back edge. It gets `back: true`.
// - The column of a node is the length of the longest path to it without back edges.
// - A column lists its nodes in address order and then the waypoints of the long edges. The
//   barycenter sweeps sort it. Its rows are ROW_GAP apart round the middle line.
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
// The box of a condition label at the text size k = WHEN_K, without the page: the text size
// of .flow-when in src/board_more.css times WHEN_K, WHEN_CHAR em for each character, WHEN_UP em
// above the baseline and WHEN_DOWN em below it, round the x of its middle.
export const WHEN_SIZE=11,WHEN_K=1.6,WHEN_CHAR=0.6,WHEN_UP=0.8,WHEN_DOWN=0.2,WHEN_STEP=18;
// The places of the condition labels {x, y, text} in edge order: a label moves down by
// WHEN_STEP while its box overlaps the box of an earlier label.
export function apartWhens(labels){
  const em=WHEN_SIZE*WHEN_K,placed=[];
  return labels.map(({x,y,text})=>{
    const half=[...text].length*WHEN_CHAR*em/2;
    const meets=p=>x-half<p.right&&p.left<x+half&&y-WHEN_UP*em<p.bottom&&p.top<y+WHEN_DOWN*em;
    while(placed.some(meets))y+=WHEN_STEP;
    placed.push({left:x-half,right:x+half,top:y-WHEN_UP*em,bottom:y+WHEN_DOWN*em});
    return {x,y,text};
  });
}
// `addresses` come in address order. Each edge is {from, to} in its stored order. A forward edge
// gets `via`: its waypoints {column, x, y} from left to right.
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
  const columns=Array.from({length:Math.max(0,...column.values())+1},()=>[]);
  for(const a of addresses)columns[column.get(a)].push(a);
  // A forward edge over k > 1 columns gets one waypoint in each column between: `via` lists
  // them. Each segment joins an item to the next item of its edge in the next column.
  const before=new Map(),after=new Map();
  const add=(map,key,value)=>{if(!map.has(key))map.set(key,[]);map.get(key).push(value);};
  const join=(a,b)=>{add(after,a,b);add(before,b,a);};
  for(const e of edges){
    if(e.back)continue;
    let last=e.from;e.via=[];
    for(let c=column.get(e.from)+1;c<column.get(e.to);c++){const w={column:c};columns[c].push(w);e.via.push(w);join(last,w);last=w;}
    join(last,e.to);
  }
  sweep(columns,before,after);
  const position=new Map();
  columns.forEach((list,c)=>list.forEach((item,i)=>{
    const place={x:c*COLUMN_GAP,y:(i-(list.length-1)/2)*ROW_GAP};
    if(typeof item==='string')position.set(item,{address:item,...place});else Object.assign(item,place);
  }));
  return {column,position};
}
// The 4 barycenter sweeps: left to right, right to left, left to right, right to left. A sweep
// gives each item of a column the mean row of its neighbours in the column before it (after it,
// right to left) and sorts the column by that value. An item without such neighbours keeps its
// row as its value. A tie keeps the current order. A row is the place of an item in its column.
function sweep(columns,before,after){
  const row=new Map();
  const number=list=>list.forEach((item,i)=>row.set(item,i));
  columns.forEach(number);
  const order=(list,side)=>{
    const value=new Map(list.map(item=>{const near=side.get(item)||[];return [item,near.length?near.reduce((s,o)=>s+row.get(o),0)/near.length:row.get(item)];}));
    list.sort((a,b)=>value.get(a)-value.get(b));number(list);
  };
  const last=columns.length-1;
  for(let pass=0;pass<4;pass++){
    if(pass%2===0)for(let c=1;c<=last;c++)order(columns[c],before);
    else for(let c=last-1;c>=0;c--)order(columns[c],after);
  }
}
