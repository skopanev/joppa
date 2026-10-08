const workspace=document.querySelector('#workspace'),revision=document.querySelector('#revision'),types=document.querySelector('#types');
let request=0,layout;
const element=(tag,text,cls)=>{const el=document.createElement(tag);el.textContent=text;if(cls)el.className=cls;return el;};
const svgElement=(tag,attributes)=>{const el=document.createElementNS('http://www.w3.org/2000/svg',tag);for(const [key,value] of Object.entries(attributes))el.setAttribute(key,value);return el;};
async function api(path){const response=await fetch('/api/'+path);const value=await response.json();if(!response.ok)throw Error(value.error||'Could not read the schema.');return value;}

function renderTree(schema){
  layout=undefined;
  const definitions=schema.types,root='@workspace';
  const nodes=new Map(),parents=new Map(),edges=[];
  nodes.set(root,{title:workspace.selectedOptions[0].textContent,label:'Workspace',className:'workspace-node'});
  const order=Object.keys(definitions).sort((a,b)=>{
    const weight={group:0,requirement:1,ac:2,task:3,document:4};
    return (definitions[a].index??Infinity)-(definitions[b].index??Infinity)||(weight[definitions[a].role]??4)-(weight[definitions[b].role]??4)||a.localeCompare(b);
  });
  const anyParents=order.filter(kind=>definitions[kind].parents.includes('*'));
  for(const kind of order){
    const type=definitions[kind];nodes.set(kind,{title:type.title,label:kind,className:''});
    if(type.root)edges.push({source:root,target:kind});
    for(const parent of type.parents.filter(parent=>parent!=='*'))edges.push({source:parent,target:kind});
    if(type.root){parents.set(kind,root);continue;}
    // A primary parent places each type once. All allowed parents get a line.
    for(const candidate of type.parents){
      if(candidate==='*')continue;
      const parent=candidate;let cursor=parent;const visited=new Set([kind]);
      while(cursor&&!visited.has(cursor)){visited.add(cursor);cursor=parents.get(cursor);}
      if(cursor||parent===kind)continue;
      parents.set(kind,parent);break;
    }
  }
  const references=[];
  for(const [kind,t] of Object.entries(definitions)){
    for(const [field,rule] of Object.entries(t.references||{})){
      references.push({
        source:kind,
        target:rule.target,
        reference:field,
        title:rule.title,
        text:t.title+" → "+(rule.target==="*"?"Any object type":definitions[rule.target]?.title||rule.target)+": "+rule.title.toLowerCase()+"."
      });
    }
  }
  edges.push(...references.filter(edge=>definitions[edge.source]&&definitions[edge.target]));
  document.querySelector('#reference-legend').hidden=!references.length;
  document.querySelector('#reference-help').replaceChildren(...references.map(edge=>element('p',edge.text,'hint')),...anyParents.map(kind=>element('p',definitions[kind].title+' can be placed inside any object type.','hint')));
  const children=new Map();for(const [child,parent] of parents){if(!children.has(parent))children.set(parent,[]);children.get(parent).push(child);}
  let row=0;const placed=new Set();
  const place=(id,depth)=>{
    if(placed.has(id))return;placed.add(id);const node=nodes.get(id);node.depth=depth;
    const branches=children.get(id)||[];
    if(!branches.length){node.row=row++;return;}
    for(const child of branches)place(child,depth+1);
    node.row=(nodes.get(branches[0]).row+nodes.get(branches.at(-1)).row)/2;
  };
  place(root,0);for(const id of nodes.keys())if(!placed.has(id))place(id,0);
  const connections=svgElement('svg',{id:'connections','aria-hidden':'true'});
  const defs=svgElement('defs',{}),marker=svgElement('marker',{id:'arrow',viewBox:'0 0 10 10',refX:9,refY:5,markerWidth:5,markerHeight:5,orient:'auto-start-reverse'});
  marker.append(svgElement('path',{d:'M 0 0 L 10 5 L 0 10 z',fill:'#82a394'}));defs.append(marker);connections.append(defs);types.replaceChildren(connections);
  const referenceMarker=marker.cloneNode(true);referenceMarker.id='reference-arrow';referenceMarker.querySelector('path').setAttribute('fill','#5593b4');defs.append(referenceMarker);
  let hovered;
  const highlight=id=>{for(const edge of connections.querySelectorAll('[data-source]')){const selected=edge.dataset.source===id||edge.dataset.target===id;edge.classList.toggle('selected',Boolean(id)&&selected);edge.classList.toggle('dimmed',Boolean(id)&&!selected);}};
  for(const [id,node] of nodes){
    const card=element('article','',('type '+node.className).trim());card.dataset.kind=id;card.tabIndex=0;
    const color=definitions[id]?.color||"#63756b";if(/^#[0-9a-f]{6}$/i.test(color||''))card.style.setProperty('--object-color',color);
    const size=definitions[id]?.title_size;
    card.style.setProperty('--object-title-size',(Number.isInteger(size)&&size>=12&&size<=48?size:18)+'px');
    card.append(element('h2',node.title),element('small',node.label));
    if(definitions[id]){
      const t=definitions[id],related=references.filter(edge=>edge.source===id||edge.target===id).map(edge=>edge.text);
      card.setAttribute('aria-label',t.title+'. Parents: '+[...(t.root?['Workspace']:[]),...t.parents.map(p=>p==='*'?'Any object type':definitions[p]?.title||p)].join(', ')+(related.length?'. '+related.join(' '):''));
      if(t.parents.includes('*'))card.append(element('small','Inside any type','schema-parent-rule'));
    }
    card.addEventListener('mouseenter',()=>{hovered=id;highlight(id);});card.addEventListener('mouseleave',()=>{hovered=undefined;highlight(document.activeElement?.dataset.kind);});
    card.addEventListener('focus',()=>highlight(id));card.addEventListener('blur',()=>highlight(hovered));node.card=card;types.append(card);
  }
  const roundedPath=points=>{
    let d=`M ${points[0][0]} ${points[0][1]}`;
    for(let i=1;i<points.length-1;i++){
      const before=points[i-1],corner=points[i],after=points[i+1];
      const a=Math.hypot(corner[0]-before[0],corner[1]-before[1]),b=Math.hypot(after[0]-corner[0],after[1]-corner[1]);
      const radius=Math.min(6,a/2,b/2);
      if(!a||!b)continue;
      d+=` L ${corner[0]+(before[0]-corner[0])*radius/a} ${corner[1]+(before[1]-corner[1])*radius/a} Q ${corner[0]} ${corner[1]} ${corner[0]+(after[0]-corner[0])*radius/b} ${corner[1]+(after[1]-corner[1])*radius/b}`;
    }
    return d+` L ${points.at(-1)[0]} ${points.at(-1)[1]}`;
  };
  layout=()=>{
    const depth=Math.max(...[...nodes.values()].map(node=>node.depth)),gap=48,padding=24;
    const width=Math.max(148,Math.min(188,(types.parentElement.clientWidth-padding*2-gap*depth)/(depth+1)));
    let height=78;for(const node of nodes.values()){node.card.style.width=width+'px';node.card.style.height='auto';height=Math.max(height,node.card.offsetHeight);}
    const step=height+44;
    for(const node of nodes.values()){node.x=padding+node.depth*(width+gap);node.y=node.row*step;}
    const clear=points=>[...nodes.values()].every(node=>points.slice(1).every((end,i)=>{
      const start=points[i],left=node.x+2,right=node.x+width-2,top=node.y+2,bottom=node.y+height-2;
      return start[0]===end[0]?start[0]<=left||start[0]>=right||Math.max(start[1],end[1])<=top||Math.min(start[1],end[1])>=bottom:start[1]<=top||start[1]>=bottom||Math.max(start[0],end[0])<=left||Math.min(start[0],end[0])>=right;
    }));
    const direct=new Set(),rails=new Map(),tracks=new Map();
    const track=(edge,end,column)=>{if(!tracks.has(column))tracks.set(column,[]);tracks.get(column).push({edge,end});};
    for(const edge of edges){
      const a=nodes.get(edge.source),b=nodes.get(edge.target);if(!a||!b||a===b)continue;
      const points=[[a.x+width/2,a.y+height],[a.x+width/2,b.y+height/2],[b.x+width+3,b.y+height/2]];
      if(edge.reference&&b.depth<a.depth&&b.y>a.y+height&&clear(points)){direct.add(edge);continue;}
      if(b.depth!==a.depth+1||edge.reference){rails.set(edge,rails.size);track(edge,'source',a.depth);track(edge,'target',b.depth>a.depth?b.depth-1:b.depth);}
    }
    const topPadding=padding+(rails.size?14+rails.size*18:0);
    const rightPadding=tracks.has(depth)||edges.some(edge=>edge.source===edge.target&&nodes.get(edge.source)?.depth===depth)?gap:padding;
    types.style.width=(padding+rightPadding+(depth+1)*width+depth*gap)+'px';types.style.height=(topPadding+padding+Math.max(1,row)*step-44)+'px';
    for(const node of nodes.values()){node.y+=topPadding;node.card.style.left=node.x+'px';node.card.style.top=node.y+'px';node.card.style.height=height+'px';}
    const port=(edge,end)=>{
      const id=edge[end],node=nodes.get(id),list=edges.filter(e=>e[end]===id&&!direct.has(e)).sort((a,b)=>nodes.get(a[end==='source'?'target':'source']).row-nodes.get(b[end==='source'?'target':'source']).row);
      return node.y+height*(list.indexOf(edge)+1)/(list.length+1);
    };
    const lane=(edge,end,column)=>{
      const list=tracks.get(column),index=list.findIndex(item=>item.edge===edge&&item.end===end);
      return padding+column*(width+gap)+width+gap*(index+1)/(list.length+1);
    };
    connections.querySelectorAll('.schema-edge,.schema-edge-label').forEach(edge=>edge.remove());
    for(const edge of edges){
      const a=nodes.get(edge.source),b=nodes.get(edge.target);if(!a||!b)continue;
      let d,label;const startY=port(edge,'source'),endY=port(edge,'target');
      if(direct.has(edge)){
        const x=a.x+width/2,y=b.y+height/2;
        d=roundedPath([[x,a.y+height],[x,y],[b.x+width+3,y]]);
        label={x:(x+b.x+width)/2,y:y-9,text:edge.title};
      }
      else if(b.depth===a.depth+1&&!edge.reference){
        const siblings=edges.filter(e=>e.source===edge.source&&nodes.get(e.target)?.depth===b.depth&&!e.reference),index=siblings.indexOf(edge);
        const startX=a.x+width,endX=b.x-3,middle=startX+gap*(index+1)/(siblings.length+1);
        d=roundedPath([[startX,startY],[middle,startY],[middle,endY],[endX,endY]]);
      }
      else if(a===b){const x=a.x+width;d=roundedPath([[x,a.y+height/3],[x+24,a.y+height/3],[x+24,a.y+height*2/3],[x+3,a.y+height*2/3]]);}
      else{
        // Each endpoint uses its own vertical track. Each edge uses its own rail.
        const startX=a.x+width,forward=b.depth>a.depth,endX=forward?b.x-3:b.x+width+3;
        const afterSource=lane(edge,'source',a.depth),beforeTarget=lane(edge,'target',forward?b.depth-1:b.depth),railY=26+rails.get(edge)*18;
        d=roundedPath([[startX,startY],[afterSource,startY],[afterSource,railY],[beforeTarget,railY],[beforeTarget,endY],[endX,endY]]);
        if(edge.reference)label={x:(afterSource+beforeTarget)/2,y:railY-5,text:edge.title};
      }
      const path=svgElement('path',{d,class:'schema-edge'+(edge.reference?' reference-edge':''),'marker-end':edge.reference?'url(#reference-arrow)':'url(#arrow)'});
      if(edge.reference)path.dataset.reference=edge.reference;
      path.dataset.source=edge.source;path.dataset.target=edge.target;connections.append(path);
      if(label){const text=svgElement('text',{x:label.x,y:label.y,class:'schema-edge-label','text-anchor':'middle'});text.textContent=label.text;text.dataset.source=edge.source;text.dataset.target=edge.target;connections.append(text);}
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
