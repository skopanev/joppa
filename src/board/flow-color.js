// The colours of the edges of Flow view: relations.<key>.color of a node and edge.color. A
// colour is a palette name, drawn with the CSS token --flow-<name> of src/board.css, or
// #rrggbb. Only edge lines and arrow heads take a colour; a dot keeps its tone.
// - Nodes resolve in column order, and in address order inside a column. A node takes its
//   own colour, else the colour of its first coloured incoming forward edge, by source address.
// - An edge takes its own colour, else the resolved colour of its source. A back edge gets a
//   colour by the same rule, and passes none on.
const PALETTE=['blue','purple','teal','amber','pink','indigo','cyan','lime'];
// The CSS value of a colour, or null for no colour and an unknown form.
export function paint(color){
  if(PALETTE.includes(color))return 'var(--flow-'+color+')';
  return /^#[0-9a-f]{6}$/i.test(color||'')?color.toLowerCase():null;
}
// Give each edge its `color`. `nodes` come in address order with their own `color`, and
// `column` gives the column of each node.
export function flowColors(nodes,edges,column){
  const index=new Map(nodes.map((n,i)=>[n.address,i])),into=new Map(),resolved=new Map();
  for(const e of edges)if(!e.back){if(!into.has(e.to))into.set(e.to,[]);into.get(e.to).push(e);}
  const colorOf=e=>(paint(e.color)&&e.color)||resolved.get(e.from)||null;
  const order=[...nodes].sort((a,b)=>column.get(a.address)-column.get(b.address)||index.get(a.address)-index.get(b.address));
  for(const node of order){
    const incoming=[...(into.get(node.address)||[])].sort((a,b)=>index.get(a.from)-index.get(b.from));
    resolved.set(node.address,(paint(node.color)&&node.color)||incoming.map(colorOf).find(Boolean)||null);
  }
  for(const e of edges)e.color=colorOf(e);
}
// The id of the arrow head marker of one colour in `defs`, made on first use from `base`, the
// shared marker #flow-arrow. Its path fill is the colour.
export function colorMarker(defs,base,color){
  const id='flow-arrow-'+color.replace('#','').toLowerCase();
  if(!defs.querySelector('#'+id)){
    const marker=base.cloneNode(true);marker.id=id;
    const head=marker.firstChild;head.removeAttribute('class');head.style.fill=paint(color);
    defs.append(marker);
  }
  return id;
}
