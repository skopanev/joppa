function statusCircle(signal,number){
  const circle=E('span',number===undefined?undefined:String(number),'status-circle '+signal.tone+(number===undefined?'':' numbered'));
  const assessment=signal.readiness_blocker||signal.assessment;
  const hint='Status: '+signal.label+assessmentHint(assessment)+(signal.at?'\nStatus event: '+eventDate(signal.at):'');
  circle.dataset.tone=signal.tone;circle.dataset.hint=hint;circle.tabIndex=0;
  circle.setAttribute('role','img');circle.setAttribute('aria-label',hint);
  circle.addEventListener('pointerenter',()=>showStatusHint(circle));
  circle.addEventListener('pointerleave',hideStatusHint);
  circle.addEventListener('focus',()=>showStatusHint(circle));
  circle.addEventListener('blur',hideStatusHint);
  return circle;
}
function assessmentHint(a){
  if(!a)return '';
  if(a.state==='dependency')return '\nWaits for: '+a.target;
  if(a.state==='unassessed')return '\nReadiness: Not assessed';
  if(a.state==='waiting')return '\nQuestion: '+a.reason+'\nRecipient: '+personLabel(a.recipient||'Unassigned');
  return '\nReadiness: Ready';
}
function nodeTitle(text,signal){
  const title=E('span',text,'node-title');title.prepend(statusCircle(signal));return title;
}
function rootCollectionTabs(root){
  if(!Object.keys(data.objectTypes||{}).length)return;
  const types=Object.entries(data.objectTypes||{}).filter(([,type])=>type.root)
    .sort(([a,left],[b,right])=>(left.index??Infinity)-(right.index??Infinity)||a.localeCompare(b));
  const existing=[...root.querySelectorAll('.root-collection')];
  for(const panel of existing)panel.hidden=true;
  if(!types.length){root.querySelector(':scope > .collection-tabs')?.remove();return;}
  const panels=types.map(([kind,type])=>{
    let panel=existing.find(node=>node.dataset.collectionKind===kind);
    if(!panel){
      panel=E('section',undefined,'root-collection');panel.dataset.collectionKind=kind;panel.dataset.collectionLazy='true';applyObjectColor(panel,kind);
      panel.append(E('header',undefined,'collection-heading'),E('div',undefined,'collection-items'));root.append(panel);
      collectionHeading(panel,type.title,type.description||'');
    }
    return panel;
  });
  let nav=root.querySelector(':scope > .collection-tabs');
  if(!nav){nav=E('div',undefined,'collection-tabs');nav.setAttribute('role','tablist');nav.setAttribute('aria-label','Root collections');root.prepend(nav);}
  nav.replaceChildren();nav.hidden=false;
  const key='joppa-collection:'+ws;
  let selected;
  try{selected=sessionStorage.getItem(key);}catch{}
  if(!panels.some(panel=>panel.dataset.collectionKind===selected))selected=panels[0]?.dataset.collectionKind;
  const activate=(kind,focus=false)=>{
    closeAccountMenu();
    selected=kind;try{sessionStorage.setItem(key,kind);}catch{}
    for(const panel of panels){
      const active=panel.dataset.collectionKind===kind;panel.hidden=!active||Boolean(data.filterActive);
      const tab=nav.querySelector('[aria-controls="'+panel.id+'"]');tab.setAttribute('aria-selected',String(active));tab.tabIndex=active?0:-1;
      if(active){
        data.activeRootKind=kind;
        if(focus)tab.focus();
        if(data.filterActive)continue;
        panel.collectionLoad?.();
        if(panel.dataset.collectionLazy==='true'){
          delete panel.dataset.collectionLazy;
          const contents=panel.querySelector(':scope > .collection-items')||panel;
          contents.append(E('p','Loading objects…','object-loading hint'));
          appendObjectChildren(contents,undefined,kind).catch(error=>{panel.dataset.collectionLazy='true';contents.replaceChildren();notify(error);});
        }
      }
    }
    if(data.filterActive)data.filterUpdate?.();
  };
  for(const panel of panels){
    const kind=panel.dataset.collectionKind,type=data.objectTypes[kind];
    panel.id='collection-'+kind;panel.setAttribute('role','tabpanel');panel.setAttribute('aria-labelledby','tab-'+kind);panel.tabIndex=-1;
    const tab=button(type.title,()=>activate(kind),'collection-tab');applyObjectColor(tab,kind);tab.id='tab-'+kind;tab.setAttribute('role','tab');tab.setAttribute('aria-controls',panel.id);nav.append(tab);
  }
  nav.onkeydown=event=>{
    const tabs=[...nav.children],current=tabs.indexOf(document.activeElement);let next;
    if(event.key==='ArrowRight')next=(current+1)%tabs.length;
    if(event.key==='ArrowLeft')next=(current+tabs.length-1)%tabs.length;
    if(event.key==='Home')next=0;if(event.key==='End')next=tabs.length-1;
    if(next!==undefined){event.preventDefault();activate(panels[next].dataset.collectionKind,true);}
  };
  activate(selected);
}
function collectionHeading(section,title,description,label){
  const summary=section.querySelector(':scope > summary,:scope > header');
  const caption=E('span',undefined,'collection-caption');
  caption.append(E('span',title,'collection-title'),E('span',description,'collection-description'));
  summary.replaceChildren(caption);
  if(label)summary.append(E('span',label,'collection-label'));
}
function displayPersonName(value){
  const name=String(value||'').trim();
  if(!name)return 'Not assigned';
  const local=name.includes('@')?name.split('@')[0].split('+')[0]:name;
  if(!/^[\p{L}]+(?:[._][\p{L}]+)+$/u.test(local))return local;
  return local.split(/[._]/).map(part=>part[0].toLocaleUpperCase()+part.slice(1)).join(' ');
}
function personLabel(actor){
  return displayPersonName(data.member_labels?.[actor]||actor);
}
function eventDate(value){
  const date=new Date(value.replace(' ','T').replace(/(\.\d{3})\d+/,'$1').replace(/([+-]\d{2})$/,'$1:00'));
  return Number.isNaN(date.getTime())?'Date unavailable':new Intl.DateTimeFormat('en-GB',{day:'numeric',month:'short',year:'numeric',hour:'2-digit',minute:'2-digit'}).format(date);
}
if (window.SNAPSHOT) {
  document.body.classList.remove('checking-session');
  document.body.classList.add('snapshot');
  $('login-screen').remove();
  $('refresh').remove();
  $('timeline-link').hidden=true;
  document.body.classList.add('authenticated');
  data = window.SNAPSHOT;
  ws = data.workspace;
  const option=E('option',data.state.title);option.value=ws;$('workspaces').replaceChildren(option);
  load().catch(notify);
}

// Browser sessions use an HttpOnly cookie; no token field or token URL.
if(!window.SNAPSHOT){
  sessionStorage.removeItem('joppa-token');
  if(location.hash.startsWith('#token='))history.replaceState(null,'',location.pathname+location.search);
  fetch('/api/me').then(async r=>{
    if(r.ok)await showApp(await r.json());
    else if(r.status!==401)throw Error('Could not restore your session. Please reload the page.');
  }).catch(e=>{$('login-status').textContent=e.message;$('login-status').dataset.error='true';})
    .finally(()=>document.body.classList.remove('checking-session'));
}
function when(at){
  if(!at)return '';
  const d=new Date(at.replace(' ','T').replace(/\+00$/,'Z'));
  return isNaN(d)?at:d.toLocaleString(undefined,{dateStyle:'medium',timeStyle:'short'});
}
function svgIcon(d,size=16){
  const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');
  for(const [k,v] of Object.entries({viewBox:'0 0 24 24',width:String(size),height:String(size),fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(k,v);
  const path=document.createElementNS(icon.namespaceURI,'path');path.setAttribute('d',d);icon.append(path);
  return icon;
}
// Кнопка с цифрой. Цифра — это и есть ответ на вопрос «тут что-то обсуждали?»,
// ради которого иначе пришлось бы открывать каждый объект по очереди.
function attachmentList(attachments){
  const list=E('ul',undefined,'files-list');
  for(const [,a] of attachments){
    const li=E('li'),link=E('a',undefined,'file-link');
    link.href='/api/workspaces/'+encodeURIComponent(ws)+'/blobs/'+a.hash;
    link.target='_blank';link.rel='noopener';
    const icon=E('span',undefined,'file-icon');icon.append(svgIcon(FILE_ICON,22));
    const info=E('span',undefined,'file-info');
    info.append(E('span',a.name,'file-name'),E('span',[fileSize(a.size),personLabel(a.actor),when(a.at)].filter(Boolean).join(' · '),'file-meta'));
    link.append(icon,info,svgIcon('M7 17 17 7 M7 7h10v10',16));
    li.append(link);list.append(li);
  }
  return list;
}
function fileSize(bytes){
  if(bytes<1024)return bytes+' B';
  if(bytes<1024*1024)return Math.round(bytes/1024)+' KB';
  return (bytes/(1024*1024)).toFixed(1)+' MB';
}
/* Common objects use the same cards for every configured type. */
async function objectPart(params){
  if(!window.SNAPSHOT)return api('workspaces/'+encodeURIComponent(ws)+'/objects?'+new URLSearchParams(params));
  const objects=data.snapshot_objects||{};
  if(params.address){
    const object=objects[params.address];if(!object)throw Error('Unknown object');
    const document=object.revisions.find(r=>r.revision===Number(params.revision||object.current_revision));if(!document)throw Error('Unknown revision');
    const historical=document.revision!==object.current_revision;
    const links=document.links.map(link=>({link,title:objects[link.target]?.revisions.at(-1).data.title,current_revision:objects[link.target]?.current_revision}));
    return {...object,document,historical,links,annotations:historical?{}:object.annotations,process:historical?null:object.process};
  }
  const presentParents=new Set(Object.values(objects).filter(o=>o.revisions.at(-1).present).map(o=>o.revisions.at(-1).parent));
  const items=Object.values(objects).filter(o=>{
    const r=o.revisions.at(-1);
    const related=params.related_to&&(r.parent===params.related_to||r.links.some(l=>l.target===params.related_to)||objects[params.related_to]?.revisions.at(-1).links.some(l=>l.target===o.address));
    return (r.present||presentParents.has(o.address))&&(!params.kind||params.kind===o.kind)&&(params.related_to?related:params.parent==='*'||(params.parent||null)===r.parent)&&(!params.note_status||(r.data.status||'unprocessed')===params.note_status);
  }).map(o=>{const r=o.revisions.at(-1);return {...o,parent:r.parent,title:r.data.title,owner:r.data.owner,revision:r.revision,at:r.at,present:r.present,note_status:r.data.status};});
  return {items,next_cursor:null};
}
function applyObjectColor(node,kind){
  const type=data.objectTypes?.[kind]||{};
  const color=type.color||'#63756b';
  if(/^#[0-9a-f]{6}$/i.test(color))node.style.setProperty('--object-color',color);
  const size=type.title_size;
  node.style.setProperty('--object-title-size',(Number.isInteger(size)&&size>=12&&size<=48?size:18)+'px');
}
function objectURL(address,revision){
  const url=new URL(location.href);url.search='';url.hash='';
  url.searchParams.set('workspace',ws);
  if(address)url.searchParams.set('object',address);
  if(revision)url.searchParams.set('revision',revision);
  return url.href;
}
function objectTitlePreview(title,kind){
  const limit=data.objectTypes?.[kind]?.title_preview_length||64,characters=Array.from(title);
  return characters.length>limit?characters.slice(0,limit-1).join('').trimEnd()+'…':title;
}
function objectLink(title,address,revision){
  const link=E('a',objectTitlePreview(title,address.split(':')[0]),'object-link');link.setAttribute('aria-label',title);link.href=objectURL(address,revision);link.dataset.linkAddress=address;return link;
}
