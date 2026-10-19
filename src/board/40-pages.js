async function renderObjectPage(root,address,revision,generation){
  const navigation=E('nav',undefined,'object-page-navigation');navigation.setAttribute('aria-label','Object navigation');
  const board=E('a','Back to board','text-button');board.href=objectURL();navigation.append(board);root.append(navigation);
  const page=E('section',undefined,'object-page');root.append(page);page.append(E('p','Loading object…','hint'));
  try{
    if(!address)throw Error('Object not found.');
    if(revision!==null&&!/^[1-9]\d*$/.test(revision))throw Error('Invalid object revision.');
    const detail=await objectPart(revision===null?{address}:{address,revision});if(generation!==loadNumber)return;
    const r=detail.document,d=r.data;
    document.title=d.title+' · Joppa';
    navigation.append(E('span','Revision '+r.revision+(detail.historical?' · Saved document':''),'hint'));
    if(r.parent)navigation.append(objectLink('Parent',r.parent));
    const card=commonObjectCard({...detail,title:d.title,owner:d.owner,revision:r.revision,parent:r.parent,present:r.present,at:r.at,note_status:d.status},detail);
    page.replaceChildren(card);card.open=true;
  }catch(e){if(generation===loadNumber&&page.isConnected)page.replaceChildren(E('p',e.message,'empty-state'));}
}
/// The workspace timeline, newest change first. A row with an object revision opens that revision.
async function renderTimelinePage(root,generation){
  const navigation=E('nav',undefined,'object-page-navigation');navigation.setAttribute('aria-label','Timeline navigation');
  const board=E('a','Back to board','text-button');board.href=objectURL();navigation.append(board);
  const page=E('section',undefined,'timeline-page');page.append(E('h2','Timeline','timeline-title'));
  const list=E('ol',undefined,'timeline-list');page.append(list);root.append(navigation,page);
  document.title='Timeline · Joppa';
  const more=async cursor=>{
    const params=new URLSearchParams({order:'newest',limit:'50'});if(cursor)params.set('cursor',cursor);
    const part=await api('workspaces/'+encodeURIComponent(ws)+'/timeline?'+params);if(generation!==loadNumber||!page.isConnected)return;
    page.querySelector(':scope > .timeline-more')?.remove();
    for(const item of part.items)list.append(timelineRow(item));
    if(!list.children.length)page.append(E('p','No changes yet.','hint'));
    if(part.next_cursor){const next=button('Load older changes',async()=>{next.disabled=true;try{await more(part.next_cursor);}catch(e){next.disabled=false;notify(e);}},'text-button timeline-more');page.append(next);}
  };
  try{await more();}catch(e){if(generation===loadNumber&&page.isConnected)page.append(E('p',e.message,'empty-state'));}
}
function timelineRow(item){
  const row=E('li',undefined,'timeline-entry');row.dataset.position=item.position;row.dataset.change=item.change;
  const kind=item.kind||String(item.address||'').split(':')[0];
  const title=item.title||item.address||item.op;
  const target=item.address&&item.revision?E('a',undefined,'timeline-row'):E('div',undefined,'timeline-row');
  if(target.tagName==='A'){target.href=objectURL(item.address,item.revision);target.dataset.linkAddress=item.address;}
  const time=E('time',eventDate(item.at),'timeline-time');time.dateTime=item.at;
  target.append(time,E('span',personLabel(item.actor),'timeline-actor'),E('span',objectTitlePreview(title,kind),'timeline-object'),E('span',item.change,'timeline-change'));
  target.title=[item.change,title,item.reason].filter(Boolean).join(' · ');
  row.append(target);return row;
}
async function appendObjectChildren(root,parent,kind,extra={}){
  const generation=loadNumber;
  const params={limit:'50',...extra};if(parent)params.parent=parent;if(kind)params.kind=kind;
  const part=await objectPart(params);if(generation!==loadNumber||!root.isConnected)return;
  root.querySelector(':scope > .object-loading')?.remove();
  appendObjectPage(root,part,params,generation);
}
function appendObjectPage(root,part,params,generation){
  const items=part.items;
  const ancestors=new Set();
  for(let node=root;node;node=node.parentElement){
    const address=node.dataset.objectAddress||node.dataset.objectContextAddress;
    if(address)ancestors.add(address);
  }
  for(const item of items){
    if(ancestors.has(item.address))continue;
    // A retired Task is history: the timeline and its own page keep it, the tree does not.
    if(item.process?.status==='retired')continue;
    let holder=root;
    if(!params.parent&&!params.kind){
      holder=[...root.querySelectorAll(':scope > .object-collection')].find(group=>group.dataset.kind===item.kind);
      if(!holder){
        holder=E('section',undefined,'object-collection root-collection');holder.dataset.kind=item.kind;holder.dataset.collectionKind=item.kind;applyObjectColor(holder,item.kind);
        holder.append(E('h2',data.objectTypes[item.kind].title,'object-collection-title'));root.append(holder);
      }
    }
    if(![...holder.querySelectorAll(':scope > .common-object')].some(card=>card.dataset.objectAddress===item.address)){
      // A load or a reload shows the top level only: no branch opens from an earlier visit.
      const card=commonObjectCard(item,undefined,root.closest('.common-object')?.dataset.objectTitle);holder.append(card);
    }
  }
  applyTestStatuses(items);
  if(!items.length&&!part.next_cursor&&!params.parent&&!root.querySelector('.common-object')){
    const type=params.kind&&data.objectTypes[params.kind];
    root.append(E('p',type?'No '+type.title.toLowerCase()+' here.':'No objects here yet.','hint collection-empty'));
  }
  if(part.next_cursor){const more=button('Load more',async()=>{more.disabled=true;try{const next=await objectPart({...params,cursor:part.next_cursor});if(generation!==loadNumber||!root.isConnected)return;more.remove();appendObjectPage(root,next,params,generation);}catch(e){more.disabled=false;notify(e);}},'text-button');root.append(more);}
  if(!params.parent&&!params.kind&&$('tree').querySelector('.collection-tabs'))rootCollectionTabs($('tree'));
}
async function renderObjectTree(){
  const root=$('tree');root.replaceChildren();
  const generation=loadNumber;
  if(window.SNAPSHOT)data.objectTypes=data.snapshot_schema.types;
  else if(!data.objectTypes||!Object.keys(data.objectTypes).length){
    const configuration=await api('workspaces/'+encodeURIComponent(ws)+'/tree');if(generation!==loadNumber)return;
    data.objectTypes=configuration.schema.types;
  }
  const selection=new URLSearchParams(location.search);
  if(selection.has('object'))return renderObjectPage(root,selection.get('object'),selection.get('revision'),generation);
  if(selection.get('view')==='timeline')return renderTimelinePage(root,generation);
  if(data.launch.configured)root.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+(data.launch.blocker_count??data.launch.blockers?.length),'hint'));
  const contents=E('div',undefined,'object-filter-results');contents.hidden=true;
  commonObjectFilters(root,contents);root.append(contents);
  for(const kind of inboxKinds().filter(kind=>data.objectTypes[kind].root))commonNotes(root,undefined,kind);
  rootCollectionTabs(root);
}
/// Every configured inbox type gets its own collection, in schema order.
function inboxKinds(){
  return Object.entries(data.objectTypes||{}).filter(([,t])=>t.inbox===true)
    .sort((a,b)=>(a[1].index??Infinity)-(b[1].index??Infinity)||a[0].localeCompare(b[0]))
    .map(([kind])=>kind);
}
/// An input queue shows unprocessed entries first. The schema declares it.
function inputQueue(kind){
  return kind?data.objectTypes[kind]?.inbox===true:false;
}
function commonObjectFilters(root,contents){
  const controls=E('div',undefined,'filter-fields');let request=0;
  const picker=(label,options)=>{const select=E('select');select.setAttribute('aria-label',label);for(const [value,title] of options){const option=E('option',title);option.value=value;select.append(option);}controls.append(select);return select;};
  const owner=picker('Object owner',[['','All owners'],...Object.keys(data.state.members).map(id=>[id,personLabel(id)])]);
  const status=picker('Object status',[['','All statuses'],['green','Checks passed'],['red','Checks failed'],['purple','Dependency review'],['yellow','Waiting or in progress'],['grey','Unchecked or completed']]);
  const update=async()=>{const mine=++request,generation=loadNumber;const selected={kind:data.activeRootKind,owner:owner.value,status:status.value};const filtering=Boolean(selected.owner||selected.status);data.filterActive=filtering;contents.hidden=!filtering;const holder=E('div');contents.replaceChildren(holder);try{
    if(!filtering){rootCollectionTabs(root);return;}
    for(const panel of root.querySelectorAll('.root-collection'))panel.hidden=true;
    holder.append(E('p','Loading objects…','object-loading hint'));
    const params={parent:'*',limit:'100'};if(selected.kind)params.kind=selected.kind;if(inputQueue(selected.kind))params.note_status='unprocessed';let cursor;const items=[];
    do{const page=await objectPart(cursor?{...params,cursor}:params);if(mine!==request||generation!==loadNumber||!controls.isConnected)return;items.push(...page.items);cursor=page.next_cursor;}while(cursor);
    const rows=items.filter(item=>(selected.kind||!inputQueue(item.kind))&&(!selected.owner||item.owner===selected.owner)&&(!selected.status||item.signal?.tone===selected.status));
    holder.replaceChildren(...rows.map(commonObjectCard));if(!rows.length)holder.append(E('p','No objects match these filters.','hint'));
  }catch(e){if(mine===request)notify(e);}};
  data.filterActive=false;data.filterUpdate=update;
  for(const select of [owner,status])select.onchange=update;root.append(controls);return controls;
}

function commonNotes(root,relatedTo,kind){
  const label=data.objectTypes?.[kind]?.title||kind;
  const title=relatedTo?'Related '+label.toLowerCase():label;
  const queue=inputQueue(kind);
  const section=E('section');section.append(E('header',undefined,'collection-heading'));root.append(section);section.classList.add('note-section');applyObjectColor(section,kind);
  collectionHeading(section,title,data.objectTypes?.[kind]?.description||(!relatedTo&&queue?'Incoming information to review.':''));
  if(!relatedTo){section.classList.add('root-collection');section.dataset.collectionKind=kind;}
  const summary=section.querySelector(':scope > header');
  const body=E('div',undefined,'note-section-body');section.append(body);
  let loaded=false,loading=false,request=0;
  let all;
  if(!relatedTo&&queue){
    const controls=E('div',undefined,'note-controls');
    const option=E('label',undefined,'note-all');all=E('input');all.type='checkbox';all.setAttribute('aria-label','All '+label.toLowerCase());option.append(all,E('span','All'));controls.append(option);summary.append(controls);
  }
  const rows=E('div',undefined,'note-items');body.append(rows);
  const loadNotes=async()=>{
    const mine=++request;loaded=false;loading=true;
    const holder=E('div');holder.append(E('p','Loading '+label.toLowerCase()+'…','object-loading hint'));rows.replaceChildren(holder);
    try{await appendObjectChildren(holder,'*',kind,relatedTo?{related_to:relatedTo}:queue&&!all?.checked?{note_status:'unprocessed'}:{});if(mine===request&&section.isConnected){
      loaded=true;const empty=!holder.querySelector('.common-object');section.classList.toggle('empty-notes',empty);if(relatedTo)section.hidden=empty;
      summary.title=empty?'No '+label.toLowerCase()+' here.':'Read '+label.toLowerCase();
    }}
    catch(e){if(mine===request){loaded=false;holder.replaceChildren();notify(e);}}
    finally{if(mine===request)loading=false;}
  };
  if(all)all.onchange=loadNotes;
  section.collectionLoad=()=>{if(!loaded&&!loading)loadNotes();};
  if(relatedTo){
    const parent=section.parentElement.closest('details');
    if(!parent||parent.open)section.collectionLoad();
    else parent.addEventListener('toggle',()=>{if(parent.open)section.collectionLoad();});
  }
  return section;
}
// A type badge is a plain label: a click goes to its row, and no popover opens.
function objectTypeLabel(kind){
  const type=data.objectTypes?.[kind]||{},title=type.title||kind.toUpperCase();
  const words=title.trim().split(/\s+/);
  const short=words.length===1&&title===title.toLocaleUpperCase()?title:words.map(word=>Array.from(word)[0]).join('').toLocaleUpperCase();
  const question=type.question||'',hint=question?title+'\n'+question:title;
  const label=E('abbr',type.badge||short,'type-label type-badge');
  label.dataset.hint=hint;label.title=hint;label.setAttribute('aria-label',title);
  label.addEventListener('pointerenter',()=>showStatusHint(label));
  label.addEventListener('pointerleave',hideStatusHint);
  return label;
}
// Open branches survive a visit to an object page and the way back to the board.
// Object text is Markdown. Line breaks stay, active HTML is removed, and external links open safely.
const markdownSafe=(()=>{
  if(!window.DOMPurify||!window.marked)return null;
  DOMPurify.addHook('afterSanitizeAttributes',node=>{
    if(node.tagName==='A'){
      const href=node.getAttribute('href')||'';
      if(/^\s*(javascript|data|vbscript):/i.test(href))node.removeAttribute('href');
      else if(/^https?:/i.test(href)){node.setAttribute('target','_blank');node.setAttribute('rel','noopener noreferrer');}
    }
    if(node.tagName==='INPUT')node.setAttribute('disabled','');
  });
  return text=>DOMPurify.sanitize(marked.parse(text,{gfm:true,breaks:true}),
    {FORBID_TAGS:['style','iframe','svg','math','form','object','embed'],FORBID_ATTR:['style']});
})();
function markdownBlock(text,cls){
  const box=E('div',undefined,'markdown '+cls);
  if(markdownSafe)box.innerHTML=markdownSafe(text);else{box.textContent=text;box.classList.add('markdown-raw');}
  return box;
}
// Every live node shows its priority read-only in the row.
function priorityBadge(item){
  const badge=E('span',item.process.priority.toUpperCase(),'priority-badge');
  badge.setAttribute('aria-label','Priority of '+item.title);
  return badge;
}
