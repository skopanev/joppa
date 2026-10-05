const workspace=document.querySelector('#workspace'),revision=document.querySelector('#revision'),types=document.querySelector('#types');
let request=0,layout;
const element=(tag,text,cls)=>{const el=document.createElement(tag);el.textContent=text;if(cls)el.className=cls;return el;};
const svgElement=(tag,attributes)=>{const el=document.createElementNS('http://www.w3.org/2000/svg',tag);for(const [key,value] of Object.entries(attributes))el.setAttribute(key,value);return el;};
async function api(path){const response=await fetch('/api/'+path);const value=await response.json();if(!response.ok)throw Error(value.error||'Could not read the schema.');return value;}

function renderTree(schema){
  layout=undefined;
  const definitions=schema.types,root='@workspace',any='@any';
  const nodes=new Map(),parents=new Map(),edges=[];
  nodes.set(root,{title:workspace.selectedOptions[0].textContent,label:'Workspace',className:'workspace-node'});
  const order=Object.keys(definitions).sort((a,b)=>{
    const weight={group:0,requirement:1,ac:2,task:3,document:4};
    return (weight[definitions[a].role]??4)-(weight[definitions[b].role]??4)||a.localeCompare(b);
  });
  const hasAny=order.some(kind=>definitions[kind].parents.includes('*'));
  document.querySelector('#any-legend').hidden=!hasAny;
  if(hasAny)nodes.set(any,{title:'Any object',label:'Any type can be a parent',className:'any-node'});
  for(const kind of order){
    const type=definitions[kind];nodes.set(kind,{title:type.title,label:kind,className:''});
    if(type.root)edges.push({source:root,target:kind});
    for(const parent of type.parents)edges.push({source:parent==='*'?any:parent,target:kind,any:parent==='*'});
    if(type.root){parents.set(kind,root);continue;}
    // A primary parent places each type once. All allowed parents get a line.
    for(const candidate of type.parents){
      const parent=candidate==='*'?any:candidate;let cursor=parent;const visited=new Set([kind]);
      while(cursor&&!visited.has(cursor)){visited.add(cursor);cursor=parents.get(cursor);}
      if(cursor||parent===kind)continue;
      parents.set(kind,parent);break;
    }
  }
  const references=[
    {source:'requirement',target:'consumer',reference:'consumers',text:'REQ → Consumer: who uses the requirement.'},
    {source:'task',target:'project',reference:'projects',text:'Task → Project: which repository the work affects.'}
  ].filter(edge=>definitions[edge.source]&&definitions[edge.target]);
  edges.push(...references);
  document.querySelector('#reference-legend').hidden=!references.length;
  document.querySelector('#reference-help').replaceChildren(...references.map(edge=>element('p',edge.text,'hint')));
  const children=new Map();for(const [child,parent] of parents){if(!children.has(parent))children.set(parent,[]);children.get(parent).push(child);}
  let row=0;const placed=new Set();
  const place=(id,depth)=>{
    if(placed.has(id))return;placed.add(id);const node=nodes.get(id);node.depth=depth;
    const branches=children.get(id)||[];
    if(!branches.length){node.row=row++;return;}
    for(const child of branches)place(child,depth+1);
    node.row=(nodes.get(branches[0]).row+nodes.get(branches.at(-1)).row)/2;
  };
  place(root,0);if(nodes.has(any))place(any,0);for(const id of nodes.keys())if(!placed.has(id))place(id,0);
  const connections=svgElement('svg',{id:'connections','aria-hidden':'true'});
  const defs=svgElement('defs',{}),marker=svgElement('marker',{id:'arrow',viewBox:'0 0 10 10',refX:9,refY:5,markerWidth:5,markerHeight:5,orient:'auto-start-reverse'});
  marker.append(svgElement('path',{d:'M 0 0 L 10 5 L 0 10 z',fill:'#82a394'}));defs.append(marker);connections.append(defs);types.replaceChildren(connections);
  const referenceMarker=marker.cloneNode(true);referenceMarker.id='reference-arrow';referenceMarker.querySelector('path').setAttribute('fill','#5593b4');defs.append(referenceMarker);
  let hovered;
  const highlight=id=>{for(const edge of connections.querySelectorAll('.schema-edge')){const selected=edge.dataset.source===id||edge.dataset.target===id;edge.classList.toggle('selected',Boolean(id)&&selected);edge.classList.toggle('dimmed',Boolean(id)&&!selected);}};
  for(const [id,node] of nodes){
    const card=element('article','',('type '+node.className).trim());card.dataset.kind=id;card.tabIndex=0;
    card.append(element('h2',node.title),element('small',node.label));
    if(definitions[id]){
      const t=definitions[id],related=references.filter(edge=>edge.source===id||edge.target===id).map(edge=>edge.text);
      card.setAttribute('aria-label',t.title+'. Parents: '+[...(t.root?['Workspace']:[]),...t.parents.map(p=>p==='*'?'Any object':definitions[p]?.title||p)].join(', ')+(related.length?'. '+related.join(' '):''));
    }
    card.addEventListener('mouseenter',()=>{hovered=id;highlight(id);});card.addEventListener('mouseleave',()=>{hovered=undefined;highlight(document.activeElement?.dataset.kind);});
    card.addEventListener('focus',()=>highlight(id));card.addEventListener('blur',()=>highlight(hovered));node.card=card;types.append(card);
  }
  const rails=new Map();
  for(const edge of edges){const a=nodes.get(edge.source),b=nodes.get(edge.target);if(a&&b&&a!==b&&b.depth!==a.depth+1)rails.set(edge,rails.size);}
  layout=()=>{
    const depth=Math.max(...[...nodes.values()].map(node=>node.depth)),gap=48,padding=24;
    const topPadding=padding+rails.size*8;
    const width=Math.max(148,Math.min(188,(types.parentElement.clientWidth-padding*2-gap*depth)/(depth+1)));
    let height=78;for(const node of nodes.values()){node.card.style.width=width+'px';node.card.style.height='auto';height=Math.max(height,node.card.offsetHeight);}
    const step=height+26;
    types.style.width=(padding*2+(depth+1)*width+depth*gap)+'px';types.style.height=(topPadding+padding+Math.max(1,row)*step-26)+'px';
    for(const node of nodes.values()){node.x=padding+node.depth*(width+gap);node.y=topPadding+node.row*step;node.card.style.left=node.x+'px';node.card.style.top=node.y+'px';node.card.style.height=height+'px';}
    connections.querySelectorAll('.schema-edge').forEach(edge=>edge.remove());
    for(const edge of edges){
      const a=nodes.get(edge.source),b=nodes.get(edge.target);if(!a||!b)continue;
      let d;const startY=a.y+height/2,endY=b.y+height/2;
      if(b.depth===a.depth+1){const startX=a.x+width,endX=b.x-3,middle=(startX+endX)/2;d=`M ${startX} ${startY} C ${middle} ${startY}, ${middle} ${endY}, ${endX} ${endY}`;}
      else if(a===b){const x=a.x+width;d=`M ${x} ${startY-12} C ${x+28} ${startY-12}, ${x+28} ${startY+12}, ${x+3} ${startY+12}`;}
      else{
        // Extra parents use the gaps between columns and the top rail.
        const startX=a.x+width,forward=b.depth>a.depth,endX=forward?b.x-3:b.x+width+3;
        const beforeTarget=forward?b.x-12:b.x+width+12;
        const railY=12+rails.get(edge)*8;
        d=`M ${startX} ${startY} H ${startX+12} V ${railY} H ${beforeTarget} V ${endY} H ${endX}`;
      }
      const path=svgElement('path',{d,class:'schema-edge'+(edge.any?' any-edge':'')+(edge.reference?' reference-edge':''),'marker-end':edge.reference?'url(#reference-arrow)':'url(#arrow)'});
      if(edge.reference)path.dataset.reference=edge.reference;
      path.dataset.source=edge.source;path.dataset.target=edge.target;connections.append(path);
    }
    highlight(hovered||document.activeElement?.dataset.kind);
  };
  layout();types.parentElement.scrollLeft=0;
}
new ResizeObserver(()=>layout?.()).observe(types.parentElement);
async function load(selected){
  const mine=++request;document.querySelector('#error').textContent='';
  const path='workspaces/'+encodeURIComponent(workspace.value)+'/tree'+(selected===undefined?'':'?revision='+selected);
  try{
    const data=await api(path);if(mine!==request)return;revision.replaceChildren();
    for(let n=data.current_revision;n>=0;n--){const option=element('option','Revision '+n+(n===data.current_revision?' · Current':''));option.value=n;revision.append(option);}revision.value=data.schema.revision;
    document.querySelector('#meta').textContent=data.schema.reason+' · '+data.schema.actor+(data.schema.at?' · '+new Date(data.schema.at).toLocaleString():'');
    renderTree(data.schema);
  }catch(e){if(mine===request)document.querySelector('#error').textContent=e.message;}
}
workspace.onchange=()=>{sessionStorage.setItem('joppa-workspace',workspace.value);history.replaceState(null,'','/tree?workspace='+encodeURIComponent(workspace.value));load();};revision.onchange=()=>load(Number(revision.value));
(async()=>{try{const list=await api('workspaces');for(const w of list){const option=element('option',w.title);option.value=w.id;workspace.append(option);}const selected=new URLSearchParams(location.search).get('workspace')||sessionStorage.getItem('joppa-workspace');if(list.some(w=>w.id===selected))workspace.value=selected;if(list.length)await load();else document.querySelector('#error').textContent='No workspace access. Ask the workspace owner for access.';}catch(e){document.querySelector('#error').textContent=e.message+' Sign in on the main page.';}})();
