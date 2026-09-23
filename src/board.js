/* SNAPSHOT */
const FILE_ICON='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6 M8 13h8 M8 17h5';
let ws = '', data = null, viewerActor = '';
let filterMenu=null;
const $ = id => document.getElementById(id),
  E = (tag, text, cls) => {
    const e = document.createElement(tag);
    if (text !== undefined) e.textContent = text;
    if (cls) e.className = cls;
    return e;
  };
let noticeTimer;
function notify(e) {
  clearTimeout(noticeTimer);
  $('notice').textContent=e?.message||String(e||'');
  $('notice').dataset.kind=e instanceof Error?'error':'success';
  if(e && !(e instanceof Error))noticeTimer=setTimeout(()=>{$('notice').textContent='';},4500);
}
async function api(path) {
  const r = await fetch('/api/' + path);
  const j = await r.json();
  if(r.status===401){loadNumber++;data=null;document.body.classList.remove('authenticated');$('tree').replaceChildren();$('login-status').textContent='Your session expired. Please sign in again.';}
  if(r.status===404 && path.startsWith('workspaces/')){data=null;$('tree').replaceChildren(E('p','This workspace is no longer available. Choose another workspace or ask its owner for access.','empty-state'));}
  if (!r.ok) throw Error(j.error || JSON.stringify(j));
  return j;
}
function button(text, fn, cls = '') {
  const b = E('button', text, cls);
  b.type = 'button';
  b.onclick = () => Promise.resolve().then(fn).catch(notify);
  return b;
}
function details(parent, title) {
  const d = E('details');
  d.append(E('summary', title));
  parent.append(d);
  return d;
}
async function list() {
  const list = await api('workspaces');
  $('workspaces').replaceChildren();
  // Выбирать не из чего — значит и выбора нет. При одном воркспейсе список
  // остаётся видимым (человек должен знать, где он), но перестаёт быть
  // элементом управления: щелчок по нему не обещает того, чего не случится.
  $('workspaces').disabled = list.length <= 1;
  if (!list.length) {const o=E('option','No workspaces yet');o.value='';$('workspaces').append(o);}
  for (const w of list) {
    const o = E('option', w.title);
    o.value = w.id;
    $('workspaces').append(o);
  }
  if (!list.some(w => w.id === ws)) ws = list.find(w=>w.id===sessionStorage.getItem('joppa-workspace'))?.id || list[0]?.id || '';
  $('workspaces').value = ws;
  if (ws) await load();
  else {data=null;$('tree').replaceChildren(E('h1','No workspace access'),E('p','Ask your workspace owner to grant access.'));}
}
let loadNumber = 0;
async function load() {
  const request = ++loadNumber;
  const next = window.SNAPSHOT || (await api('workspaces/' + encodeURIComponent(ws)));
  if (request !== loadNumber) return;
  data = next;
  render();
}
async function showApp(identity) {
  if(identity.email===undefined)identity={...identity,...await api('me')};
  viewerActor=identity.actor;
  $('identity').textContent=identity.email||'Account';
  document.body.classList.add('authenticated');
  $('logout').hidden=false;
  await list();
}
let signInAttempt = 0;
let signInWindow = null;
$('login-cancel').onclick=()=>{
  signInAttempt++;signInWindow?.close();$('login-button').disabled=false;
  $('login-cancel').hidden=true;$('login-status').textContent='';
};
$('login-button').onclick=async()=>{
  const attempt=++signInAttempt;
  const b=$('login-button'), status=$('login-status');b.disabled=true;status.dataset.error='false';
  status.textContent='Opening Google…';
  // Open synchronously from the click so browsers do not block the sign-in window.
  signInWindow=window.open('about:blank','joppa-google','popup,width=520,height=700');
  if(signInWindow)signInWindow.opener=null;
  try {
    const r=await fetch('/auth/login',{method:'POST'});const start=await r.json();
    if(!r.ok)throw Error(start.error||'Could not start sign-in.');
    if(attempt!==signInAttempt)return;
    if(!signInWindow)throw Error('Allow the Google sign-in window, then try again.');
    signInWindow.location=start.verification_url;
    status.textContent='Complete sign-in in the Google window.';$('login-cancel').hidden=false;
    const deadline=Date.now()+start.expires_in*1000;
    while(attempt===signInAttempt && Date.now()<deadline){
      await new Promise(resolve=>setTimeout(resolve,1800));
      if(attempt!==signInAttempt)return;
      const response=await fetch('/auth/poll',{method:'POST'}), result=await response.json();
      if(attempt!==signInAttempt)return;
      if(!response.ok)throw Error(result.error||'Sign-in failed. Please try again.');
      if(result.status==='ready'){signInWindow?.close();await showApp(result);return;}
      // Google may sever the popup reference via COOP; the server decides completion.
    }
    if(attempt===signInAttempt)throw Error('Sign-in expired. Please try again.');
  }catch(e){if(attempt===signInAttempt){status.textContent=e.message;status.dataset.error='true';signInWindow?.close();}}
  finally{if(attempt===signInAttempt){b.disabled=false;$('login-cancel').hidden=true;}}
};
$('logout').onclick=async()=>{await fetch('/auth/logout',{method:'POST'});sessionStorage.removeItem('joppa-token');location.reload();};
$('workspaces').onchange=async e=>{
  ws=e.target.value;sessionStorage.setItem('joppa-workspace',ws);
$('tree').replaceChildren();
  try{await load();}catch(e){notify(e);}
};
$('refresh').onclick = () => load().catch(notify);
const statusLabels={waiting:'Waiting for decision',available:'Ready to work',progress:'In progress',ready:'Awaiting acceptance',pass:'Accepted',fail:'Checks failed'};
let filters={domain:'',status:'',person:''};
let filterWorkspace=null,statusCounts={};
function requirementStatus(id) {
  const r=data.state.requirements[id].revisions.at(-1),s=data.summary[id];
  if(Object.values(s.acs).some(a=>a.verification==='fail'))return 'fail';
  if(s.accepted)return 'pass';
  if(s.ready_for_acceptance)return 'ready';
  if(!r.confirmed||!r.acs.length||Object.values(s.acs).some(a=>!a.ready))return 'waiting';
  if(Object.keys(r.runs).length||Object.values(r.tasks).some(t=>t.purpose!=='decision'))return 'progress';
  return 'available';
}
function requirementSignal(id){
  if(!data.state.requirements[id].revisions.at(-1).acs.length)return {tone:'grey',label:'No acceptance criteria'};
  const status=requirementStatus(id);
  return {tone:status==='fail'?'red':status==='pass'?'green':status==='available'?'grey':'yellow',label:statusLabels[status]};
}
function aggregateSignal(children,noun){
  if(!children.length)return {tone:'grey',label:'No '+noun};
  const counts={green:0,yellow:0,red:0,grey:0};
  for(const child of children)counts[child.tone]++;
  const tone=counts.red?'red':counts.green===children.length?'green':counts.grey===children.length?'grey':'yellow';
  const parts=[[counts.green,'complete'],[counts.yellow,'pending'],[counts.red,'failed'],[counts.grey,'unchecked or empty']].filter(([n])=>n).map(([n,label])=>n+' '+label);
  return {tone,label:'Current '+noun+': '+parts.join(' · ')};
}
function treeSignals(){
  const requirements=Object.fromEntries(Object.keys(data.state.requirements).map(id=>[id,requirementSignal(id)]));
  const capabilities=Object.fromEntries(Object.keys(data.state.capabilities).map(id=>[id,aggregateSignal(Object.entries(data.state.requirements).filter(([,q])=>q.capability===id).map(([rid])=>requirements[rid]),'requirements')]));
  const domains=Object.fromEntries(Object.keys(data.state.domains).map(id=>[id,aggregateSignal(Object.entries(data.state.capabilities).filter(([,c])=>c.domain===id).map(([cid])=>capabilities[cid]),'capabilities')]));
  return {requirements,capabilities,domains};
}
function acSignal(status,r,id){
  if(!status)return {tone:'grey',label:'Historical revision'};
  if(status.verification==='fail')return {tone:'red',label:'Checks failed'};
  if(status.verification==='pass')return {tone:'green',label:'Checks passed'};
  if(!status.ready)return {tone:'yellow',label:'Waiting for decision'};
  const working=Object.values(r.tasks).some(t=>t.acs.includes(id)&&t.done===null)||Object.values(r.runs).some(run=>!run.finished&&run.plan.some(check=>check.ac===id));
  return {tone:working?'yellow':'grey',label:working?'In progress':'Ready to work · Not checked'};
}
function statusCircle(signal,number){
  const circle=E('span',number===undefined?undefined:String(number),'status-circle '+signal.tone+(number===undefined?'':' numbered'));
  circle.dataset.tone=signal.tone;circle.setAttribute('role','img');circle.setAttribute('aria-label',signal.label);circle.title=signal.label;
  return circle;
}
function nodeTitle(text,signal){
  const title=E('span',text,'node-title');title.prepend(statusCircle(signal));return title;
}
function matchesPerson(id) {
  const actor=filters.person==='@me'?viewerActor:filters.person;
  if(!actor)return true;
  const r=data.state.requirements[id].revisions.at(-1);
  return r.owner===actor||Object.values(r.tasks).some(t=>!t.done&&t.assignee===actor)||
    r.acs.some(a=>!data.summary[id].acs[a.id]?.ready&&(a.decider===actor||a.assessment?.recipient===actor));
}
function closeFilterMenu(restore=false){
  if(!filterMenu)return;
  const {menu,trigger}=filterMenu;menu.remove();trigger.setAttribute('aria-expanded','false');filterMenu=null;
  if(restore)trigger.focus();
}
document.addEventListener('click',e=>{if(filterMenu&&!filterMenu.host.contains(e.target))closeFilterMenu();});
document.addEventListener('keydown',e=>{if(e.key==='Escape'&&filterMenu){e.preventDefault();closeFilterMenu(true);}});
function filterPicker(kind,label,caption,options,value,change){
  const host=E('div',undefined,'filter-picker '+kind+'-picker');
  const trigger=E('button',undefined,'filter-select');trigger.type='button';trigger.id=kind+'-filter';
  for(const [key,val] of Object.entries({role:'combobox','aria-label':label,'aria-haspopup':'listbox','aria-expanded':'false','aria-controls':kind+'-options'}))trigger.setAttribute(key,val);
  const mark=E('span',kind==='domain'?'◈':kind==='person'?'◎':'●','picker-mark');mark.setAttribute('aria-hidden','true');
  if(kind==='status')mark.className='picker-mark '+value;
  const text=E('span',undefined,'picker-text'),selected=E('span',options.find(o=>o.id===value)?.title,'picker-value');
  text.append(E('small',caption),selected);
  const chevron=E('span','⌄','picker-chevron');chevron.setAttribute('aria-hidden','true');trigger.append(mark,text,chevron);host.append(trigger);
  function open(last=false){
    closeFilterMenu();
    const menu=E('div',undefined,'filter-menu');menu.id=kind+'-options';menu.setAttribute('role','listbox');menu.setAttribute('aria-label',label);
    for(const option of options){
      const row=E('button',undefined,'filter-option'+(kind==='status'?' status-option '+(option.id||'all'):''));row.type='button';row.tabIndex=-1;row.setAttribute('role','option');row.setAttribute('aria-label',option.title);row.setAttribute('aria-selected',String(option.id===value));
      row.append(E('span',option.title),E('small',typeof option.hint==='function'?option.hint():option.hint),E('span',option.id===value?'✓':'','option-check'));
      row.onclick=()=>{value=option.id;selected.textContent=option.title;if(kind==='status')mark.className='picker-mark '+value;closeFilterMenu(true);change(value);};menu.append(row);
    }
    menu.onkeydown=e=>{
      const rows=[...menu.children],index=rows.indexOf(document.activeElement);let next;
      if(e.key==='ArrowDown')next=(index+1)%rows.length;
      if(e.key==='ArrowUp')next=(index+rows.length-1)%rows.length;
      if(e.key==='Home')next=0;if(e.key==='End')next=rows.length-1;
      if(next!==undefined){e.preventDefault();rows[next].focus();}
      if(e.key==='Tab')closeFilterMenu();
    };
    host.append(menu);filterMenu={host,menu,trigger};trigger.setAttribute('aria-expanded','true');
    (last?menu.lastElementChild:menu.querySelector('[aria-selected=true]')||menu.firstElementChild).focus();
  }
  trigger.onclick=()=>{if(filterMenu?.trigger===trigger)closeFilterMenu();else open();};
  trigger.onkeydown=e=>{if(e.key==='ArrowDown'||e.key==='ArrowUp'){e.preventDefault();open(e.key==='ArrowUp');}};
  return host;
}
function filterControls(root) {
  if(filterWorkspace!==ws){
    filterWorkspace=ws;filters={domain:'',status:'',person:''};
    try{const saved=JSON.parse(sessionStorage.getItem('joppa-filters:'+ws));
      if(saved && typeof saved.domain==='string' && typeof saved.status==='string')filters={domain:saved.domain,status:saved.status,person:typeof saved.person==='string'?saved.person:''};
    }catch{}
  }
  if(filters.domain&&(!data.state.domains[filters.domain]||data.state.domains[filters.domain].archived))filters.domain='';
  if(filters.status&&!statusLabels[filters.status])filters.status='';
  const toolbar=E('section',undefined,'tree-toolbar');toolbar.setAttribute('aria-label','Filter product tree');
  const fields=E('div',undefined,'filter-fields');
  if(filters.person&&filters.person===viewerActor)filters.person='@me';
  if(filters.person&&filters.person!=='@me'&&data.state.members[filters.person]!=='human')filters.person='';
  if(filters.person==='@me'&&!viewerActor)filters.person='';
  fields.append(filterPicker('domain','Filter by domain','Domain',[
    {id:'',title:'All domains',hint:String(orderedEntries('domain').length)},
    ...orderedEntries('domain').map(([id,d])=>({id,title:d.title,hint:String(Object.values(data.state.capabilities).filter(c=>c.domain===id).length)+' capabilities'}))
  ],filters.domain,value=>{filters.domain=value;applyFilters();}));
  const people=[{id:'',title:'Everyone',hint:'All responsibilities'}];
  if(viewerActor)people.push({id:'@me',title:'Assigned to me',hint:'My responsibilities'});
  for(const [id,role] of Object.entries(data.state.members))if(role==='human'&&id!==viewerActor)people.push({id,title:personLabel(id),hint:'Responsibilities'});
  fields.append(filterPicker('person','Filter by person','Assigned to',people,filters.person,value=>{filters.person=value;applyFilters();}));
  fields.prepend(fields.lastElementChild);
  toolbar.append(fields);
  fields.append(filterPicker('status','Filter by status','Status',[
    {id:'',title:'All statuses',hint:()=>Object.values(statusCounts).reduce((a,b)=>a+b,0)+' requirements'},
    ...Object.entries(statusLabels).map(([id,title])=>({id,title,hint:()=>(statusCounts[id]||0)+' requirements'}))
  ],filters.status,value=>{filters.status=value;applyFilters();}));
  const bottom=E('div',undefined,'filter-bottom');
  const results=E('span','','filter-results');results.id='filter-results';results.setAttribute('role','status');
  bottom.append(results);toolbar.append(bottom);root.append(toolbar);
}
function applyFilters(reveal=true) {
  if(!data)return;
  try{sessionStorage.setItem('joppa-filters:'+ws,JSON.stringify(filters));}catch{}
  const counts=Object.fromEntries(Object.keys(statusLabels).map(key=>[key,0]));
  let shown=0,visibleDomains=0;
  for(const group of $('tree').querySelectorAll('.tree-domain')){
    const did=group.dataset.domain;
    const domainAllowed=!filters.domain||filters.domain===did;
    let domainVisible=false;
    for(const cap of group.querySelectorAll('.tree-capability')){
      let capVisible=false;
      for(const row of cap.querySelectorAll('.tree-requirement')){
        const id=row.dataset.requirement;
        const match=domainAllowed&&matchesPerson(id);
        const status=requirementStatus(id);
        if(match)counts[status]++;
        row.hidden=!match||!!(filters.status&&filters.status!==status);
        if(!row.hidden){shown++;capVisible=true;if(reveal&&(filters.status||filters.domain||filters.person))row.open=true;}
      }
      if(!filters.status&&!filters.person&&domainAllowed)capVisible=true;
      cap.hidden=!capVisible;domainVisible ||= capVisible;
      if(capVisible&&reveal&&(filters.status||filters.domain||filters.person))cap.open=true;
    }
    if(!filters.status&&!filters.person&&domainAllowed)domainVisible=true;
    group.hidden=!domainVisible;
    if(domainVisible){visibleDomains++;if(reveal&&(filters.status||filters.domain||filters.person))group.open=true;}
  }
  statusCounts=counts;
  $('filter-results').textContent=shown+' of '+Object.keys(data.state.requirements).length+' requirements · '+visibleDomains+' domains';
  $('filter-empty').hidden=visibleDomains>0||!orderedEntries('domain').length;
}
function render() {
  closeFilterMenu();
  const signals=treeSignals();
  const t = $('tree');
  const opened=new Set([...t.querySelectorAll('details[data-tree-node][open]')].map(d=>d.dataset.treeNode));
  t.replaceChildren();
  const intro=E('div',undefined,'tree-intro');
  intro.append(E('h1',data.state.title));
  if(data.launch.configured)intro.append(E('p',data.launch.ready?'All launch conditions accepted':'Launch conditions remaining: '+data.launch.blockers.length,'hint'));
  t.append(intro);
  filterControls(t);
  for (const [did,d] of orderedEntries('domain')) {
    const group=E('details',undefined,'tree-domain');group.dataset.domain=did;group.dataset.treeNode='domain:'+did;group.open=opened.has(group.dataset.treeNode);
    const heading=E('summary',undefined,'title-row');
    heading.append(nodeTitle(d.title,signals.domains[did]),E('small','DOMAIN','type-label'),itemActions('domain',did,d.title));group.append(heading);
    const body=E('div',undefined,'domain-body');body.append(E('p','Decision owner: '+personLabel(d.authority),'node-meta'));
    // Описание видно сразу: граница домена — то, о чём спорят чаще всего,
    // и держать её за кнопкой значит не показывать вовсе.
    if(d.description?.trim())body.append(E('p',d.description,'node-description'));
    group.append(body);
    for(const [cid,c] of orderedEntries('capability',did)){
      const cap=E('details',undefined,'tree-capability');cap.dataset.capability=cid;cap.dataset.treeNode='capability:'+cid;cap.open=opened.has(cap.dataset.treeNode);
      const head=E('summary',undefined,'title-row');head.append(nodeTitle(c.title,signals.capabilities[cid]),E('small','CAPABILITY','type-label'),itemActions('capability',cid,c.title));cap.append(head);
      if(c.description?.trim())cap.append(E('p',c.description,'node-description'));
      const reqs=orderedEntries('requirement',cid);
      if(!reqs.length)cap.append(E('p','No requirements yet.','empty-state'));
      for(const [id,q] of reqs){
        const r=q.revisions.at(-1),tone=requirementStatus(id);
        const req=E('details',undefined,'tree-requirement '+tone);req.dataset.requirement=id;req.dataset.treeNode='requirement:'+id;req.open=opened.has(req.dataset.treeNode);
        const reqHead=E('summary',undefined,'title-row');reqHead.append(nodeTitle(r.title,signals.requirements[id]),E('small','REQ','type-label'),itemActions('requirement',id,r.title,null,r.number));
        const preview=E('span',r.body||'','requirement-preview');preview.hidden=!r.body?.trim();reqHead.append(preview);
        const content=E('div',undefined,'requirement-detail');req.append(reqHead,content);
        req.addEventListener('toggle',()=>{if(req.open&&!content.childNodes.length)requirementDetails(content,id);});
        if(req.open)requirementDetails(content,id);
        cap.append(req);
      }
      body.append(cap);
    }
    t.append(group);
  }
  if(!orderedEntries('domain').length)t.append(E('p','No domains yet. Add the product structure through MCP.','empty-state'));
  const empty=E('p','No requirements match these filters.','filter-empty');empty.id='filter-empty';empty.hidden=true;t.append(empty);
  applyFilters(false);
  intake(t);
}
function requirementDetails(root, selected, revisionNumber=null) {
  root.replaceChildren();
  const q = data.state.requirements[selected],
    r = q.revisions.find(r => r.number === revisionNumber) || q.revisions.at(-1),
    current = r === q.revisions.at(-1),
    s = current ? data.summary[selected] : null;
  root.dataset.revision=r.number;
  if(current)root.dataset.status=requirementStatus(selected);
  else delete root.dataset.status;
  const currentCircle=root.parentElement?.querySelector(':scope > summary > .node-title > .status-circle');
  if(currentCircle)currentCircle.replaceWith(statusCircle(current?requirementSignal(selected):{tone:'grey',label:'Historical revision · '+r.number}));
  if(!current)root.append(E('h3',r.title));
  // Метка живёт в заголовке, рядом с историей, и обновляется при листании
  // ревизий: иначе она показывала бы последнюю, пока на экране предыдущая.
  const chip=root.parentElement?.querySelector('.rev-chip');
  if(chip){
    chip.textContent='rev. '+r.number;
    chip.classList.toggle('past',!current);
    chip.title=current?'Current revision':'Previous revision';
  }
  const preview=root.parentElement?.querySelector(':scope > summary > .requirement-preview');
  if(preview){preview.textContent=r.body||'';preview.hidden=!r.body?.trim();}
  if(r.body?.trim())root.append(E('p',r.body,'requirement-body'));
  const reqNote=notesOf('requirement:'+selected).description;
  if(reqNote?.trim())root.append(E('p',reqNote,'node-description'));
  const base = {
    req: selected,
    revision: r.number
  };
  const acList=E('ol',undefined,'ac-list');
  acList.setAttribute('aria-label','Acceptance criteria');
  if(r.acs.length)root.append(E('h3','Acceptance criteria','ac-list-title'),acList);
  for (const a of r.acs) {
    const status = s?.acs[a.id];
    const item = E('li',undefined,'ac-item '+(status?.verification==='pass'?'pass':status?.verification==='fail'?'fail':'unverified'));
    // Заголовок одной строкой: текст условия, исход справа. Раньше значок
    // уезжал под заголовок и читался как ещё одно свойство, а не как ответ.
    const circle=statusCircle(acSignal(status,r,a.id));circle.classList.add('ac-status');item.append(circle);
    const heading=E('div',undefined,'ac-heading');
    heading.append(E('p',a.text,'ac-text'));
    heading.append(itemActions('ac',a.id,a.text,selected));
    item.append(heading);
    if(a.description?.trim())item.append(E('p',a.description,'ac-description'));
    const acNote=notesOf('ac:'+selected+'/'+a.id).description;
    if(acNote?.trim())item.append(E('p',acNote,'ac-description'));
    // Свойства — подписанными парами, а не стопкой одинаковых абзацев:
    // причина, зависимость и решающий выглядели одинаково и сливались.
    const meta=E('dl',undefined,'ac-meta');
    const meta_row=(label,value)=>{meta.append(E('dt',label),E('dd',value));};
    if(current&&!status?.ready&&a.assessment?.reason)item.append(E('p',a.assessment.reason,'ac-reason'));
    // Зависимости — короткими ссылками по номеру, а не полными текстами
    // соседних условий: строка из трёх текстов нечитаема.
    if(a.depends_on?.length){
      const refs=E('span');
      a.depends_on.forEach((id,i)=>{
        const n=r.acs.findIndex(ac=>ac.id===id);
        const label=n>=0?'#'+(n+1):'unknown';
        const link=E('button',label,'ac-ref');
        link.type='button';
        link.title=n>=0?r.acs[n].text:'Another acceptance criterion';
        link.onclick=()=>{const t=acList.children[n];if(t){t.scrollIntoView({block:'center',behavior:'smooth'});t.classList.add('flash');setTimeout(()=>t.classList.remove('flash'),900);}};
        if(i)refs.append(document.createTextNode(' '));
        refs.append(link);
      });
      meta_row('Depends on',refs);
    }
    if(a.decider)meta_row('Decided by',personLabel(a.decider));
    if(meta.children.length)item.append(meta);
    acList.append(item);
  }
  tasks(root, r);
  runs(root, r, base);
  comments(root, q, r, selected);
}

function personLabel(actor){
  if(actor===viewerActor)return 'You';
  return data.member_labels?.[actor]||{agent:'Agent',runner:'Test runner'}[data.state.members[actor]]||actor?.split('@')[0]||'Not assigned';
}
function eventDate(value){
  const date=new Date(value.replace(' ','T').replace(/(\.\d{3})\d+/,'$1').replace(/([+-]\d{2})$/,'$1:00'));
  return Number.isNaN(date.getTime())?'Date unavailable':new Intl.DateTimeFormat('en-GB',{day:'numeric',month:'short',year:'numeric',hour:'2-digit',minute:'2-digit'}).format(date);
}
function historicalCommand(kind,id,position){
  return data.history.findLast(e=>e.position<=position&&(
    kind==='requirement'?e.command.op==='revise'&&e.command.req===id:
    (e.command.op===kind||e.command.op==='edit_'+kind)&&e.command.id===id))?.command;
}
function historyTitle(kind,id,position){
  if(kind==='ac'){
    for(const event of [...data.history].reverse())if(event.position<=position&&event.command.op==='revise'){
      const ac=event.command.acs.find(a=>a.id===id);if(ac)return ac.text;
    }
  }
  return historicalCommand(kind,id,position)?.title||id;
}
function historyEvent(parent,event,q){
  const c=event.command;
  const names={set_domain_archived:c.archived?'Domain archived':'Domain restored',domain:'Domain created',capability:'Capability created',reorder_tree:'Order changed',edit_domain:'Domain updated',edit_capability:'Capability updated',confirm_tree:'Structure confirmed',revise:c.expected_revision?'Requirement updated':'Requirement created',comment:{decision:'Decision recorded',accept:'Suggestion accepted',change:'Changes requested',reject:'Suggestion declined',correction:'Reply corrected'}[c.kind]||'Comment added',assess:'Readiness reviewed',confirm:'Wording approved',task:'Task created',complete_task:'Task completed',plan:'Checks planned',result:'Check result recorded',finish:'Checks completed',accept:'Requirement accepted',revoke:'Acceptance withdrawn',issue:'Issue confirmed',resolve:'Issue resolved',withdraw_decision:'Decision withdrawn'};
  const entry=E('article',undefined,'history-entry');
  entry.append(E('time',eventDate(event.recorded_at),'history-time'),E('span',personLabel(event.actor),'history-author'));parent.append(entry);
  const body=E('div',undefined,'history-body');body.append(E('strong',names[c.op]||'Item updated','history-action'));entry.append(body);
  const fact=(label,value)=>{if(value!==undefined&&value!==null&&value!==''){const p=E('p');p.append(E('span',label,'history-label'),E('span',String(value)));body.append(p);}};
  const change=(label,before,after)=>{if(before===after)return;const block=E('span',undefined,'history-change');block.append(E('span',label+': ','history-label'));if(before)block.append(E('del',String(before),'history-before'));if(before&&after)block.append(E('span',' → '));if(after)block.append(E('ins',String(after),'history-after'));body.append(block);};
  const title=(kind,id)=>historyTitle(kind,id,event.position);
  const revision=q?.revisions.find(r=>r.number===c.revision);
  if(c.reason)fact('Reason',c.reason);
  if(c.op==='domain'||c.op==='capability'){
    fact('',c.title);if(c.domain)fact('Domain',title('domain',c.domain));if(c.authority)fact('Decision owner',personLabel(c.authority));
  }else if(c.op==='edit_domain'||c.op==='edit_capability'){
    const kind=c.op.slice(5),previous=historicalCommand(kind,c.id,event.position-1);
    change('Title',previous?.title,c.title);if(c.authority)change('Decision owner',personLabel(previous?.authority),personLabel(c.authority));
  }else if(c.op==='reorder_tree'){
    const id=parent.dataset.itemId;
    if(id&&c.ids.includes(id))body.append(E('p','Moved from position '+(c.expected.indexOf(id)+1)+' to '+(c.ids.indexOf(id)+1)+'.'));
  }else if(c.op==='revise'){
    const old=q.revisions[c.expected_revision-1];
    fact('Revision',c.expected_revision+1);change('Title',old?.title,c.title);change('Description',old?.body,c.body);
    if(old?.owner!==c.owner)change('Owner',old?personLabel(old.owner):null,personLabel(c.owner));
    const previous=historicalCommand('requirement',c.req,event.position-1);
    if(previous?.capability!==c.capability)change('Capability',previous?title('capability',previous.capability):null,title('capability',c.capability));
    for(const id of new Set([...(old?.acs||[]).map(a=>a.id),...c.acs.map(a=>a.id)])){
      if(parent.dataset.itemKind==='ac'&&id!==parent.dataset.itemId)continue;
      const before=old?.acs.find(a=>a.id===id),after=c.acs.find(a=>a.id===id);
      change(!before?'Acceptance criterion added':!after?'Acceptance criterion removed':'Acceptance criterion',before?.text,after?.text);
      if(after){
        if(before?.decider!==after.decider&&(before?.decider||after.decider))change('Decision owner · '+after.text,before?.decider?personLabel(before.decider):null,personLabel(after.decider));
        if(!!before?.before_launch!==!!after.before_launch)fact('Required before launch · '+after.text,after.before_launch?'Yes':'No');
        if(JSON.stringify(before?.depends_on||[])!==JSON.stringify(after.depends_on||[]))change('Dependencies · '+after.text,before?.depends_on?.map(id=>historyTitle('ac',id,event.position-1)).join('; '),after.depends_on?.map(id=>title('ac',id)).join('; ')||'None');
      }
    }
  }else if(c.op==='comment'){
    fact('Comment',c.text);if(c.ac)fact('Acceptance criterion',title('ac',c.ac));
    if(c.reply_to)fact('In reply to',q?.comments[c.reply_to]?.text);
  }else if(c.op==='assess'){
    fact('Acceptance criterion',title('ac',c.ac));fact('Outcome',c.assessment.ready?'Ready to work':'Waiting for a decision');fact('Explanation',c.assessment.reason);
    if(c.assessment.recipient)fact('Waiting for',personLabel(c.assessment.recipient));
  }else if(c.op==='task'){
    fact('Task',c.title);fact('Assigned to',personLabel(c.assignee));fact('Acceptance criteria',c.acs.map(id=>title('ac',id)).join('; '));
  }else if(c.op==='complete_task'){
    fact('Task',revision?.tasks[c.id]?.title);fact('Result',c.evidence);
  }else if(c.op==='plan'){
    fact('Scope',c.scope);fact('Checks',c.checks.length);
    for(const check of c.checks){fact(check.method==='manual'?'Manual check':'Automated check',title('ac',check.ac));fact('Expected',check.expected.join(', '));}
  }else if(c.op==='result'){
    const check=revision?.runs[c.run]?.plan.find(p=>p.id===c.evidence.check);
    if(check){fact('Acceptance criterion',title('ac',check.ac));fact('Method',check.method==='manual'?'Manual check':'Automated test');fact('Expected',check.expected.join(', '));}
    fact('Result',{pass:'Passed',fail:'Failed',skip:'Not checked'}[c.evidence.outcome]);fact('Observed',c.evidence.observed.join(', '));fact('Note',c.evidence.note);fact('Evidence',c.evidence.link);
  }else if(c.op==='issue'||c.op==='resolve'||c.op==='withdraw_decision'){
    if(c.ac)fact('Acceptance criterion',title('ac',c.ac));fact('Discussion',q?.comments[c.comment]?.text);
  }else if(c.op==='confirm_tree')fact('Decision','The product structure was approved.');
  if(c.revision)fact('Revision',c.revision);
}
function cardFact(root,label,value){
  if(value===undefined||value===null||value==='')return;
  const line=E('p',undefined,'detail-fact');
  line.append(E('span',label+': ','hint'),E('span',value));root.append(line);
}
function acTitle(r,id){return r.acs.find(a=>a.id===id)?.text||'Acceptance criterion';}
function tasks(root, r) {
  if(!Object.keys(r.tasks).length)return;
  const d=details(root,'Tasks · '+Object.keys(r.tasks).length);
  for(const t of Object.values(r.tasks)){
    const item=E('article',undefined,'detail-entry');
    const heading=E('h3',t.title);heading.prepend(statusCircle({tone:t.done!==null?'green':'yellow',label:t.done!==null?'Task completed':'In progress'}));
    item.append(heading,E('p',personLabel(t.assignee),'hint'));
    cardFact(item,'Acceptance criteria',t.acs.map(id=>acTitle(r,id)).join('; '));
    cardFact(item,'Projects',t.projects.map(id=>data.state.projects[id]||id).join(', '));
    cardFact(item,'Work',({implementation:'Implementation',manual:'Manual action',decision:'Decision'})[t.purpose]);
    cardFact(item,'Result',t.done);d.append(item);
  }
}
function runs(root, r, base) {
  if(!Object.keys(r.runs).length)return;
  const d=details(root,'Checks · '+Object.keys(r.runs).length);
  for(const [id,run] of Object.entries(r.runs)){
    const block=details(d,run.scope+' · '+(run.finished?'Finished':'Running'));
    const failed=Object.values(run.results).some(result=>result.outcome==='fail');
    const passed=run.finished&&run.discovery_complete&&run.plan.length&&run.plan.every(check=>run.results[check.id]?.outcome==='pass');
    block.querySelector('summary').prepend(statusCircle({tone:failed?'red':passed?'green':!run.finished?'yellow':'grey',label:failed?'Checks failed':passed?'Checks passed':!run.finished?'Checks running':'Checks incomplete'}));
    for(const [component,version] of Object.entries(run.subject))cardFact(block,data.state.projects[component]||component,version);
    if(!run.discovery_complete)block.append(E('p','Check discovery is incomplete.','hint'));
    for(const c of run.plan){
      const result=run.results[c.id];
      const recorded=data.history.find(e=>e.command.op==='result'&&e.command.req===base.req&&e.command.revision===base.revision&&e.command.run===id&&e.command.evidence.check===c.id);
      const item=E('article',undefined,'detail-entry');
      item.append(E('h3',acTitle(r,c.ac)));
      const method=c.method==='manual'?'Manual check':'Automated test';
      const outcome=result?({pass:'Passed',fail:'Failed',skip:'Skipped'})[result.outcome]:'Awaiting result';
      const checkStatus=E('p',method+' · '+outcome,'hint');
      checkStatus.prepend(statusCircle({tone:result?.outcome==='pass'?'green':result?.outcome==='fail'?'red':'grey',label:outcome}));item.append(checkStatus);
      if(recorded)item.append(E('p',eventDate(recorded.recorded_at)+' · '+personLabel(recorded.actor),'hint'));
      cardFact(item,'Expected',c.expected.join(', '));
      if(result){
        cardFact(item,'Observed',result.observed.join(', ')||'Nothing recorded');
        cardFact(item,'Note',result.note);cardFact(item,'Evidence',result.link);
      }
      block.append(item);
    }
  }
}
function comments(root, q, r, reqId) {
  const entries=Object.entries(q.comments).filter(([,c])=>c.revision===r.number);
  if(!entries.length)return;
  const d=details(root,'Discussion · '+entries.length);
  const kinds={comment:'Comment',suggestion:'Suggestion',decision:'Decision',accept:'Suggestion accepted',change:'Changes requested',reject:'Suggestion declined',correction:'Correction'};
  for(const [id,c] of entries){
    const item=E('article',undefined,'detail-entry');
    const event=data.history.find(e=>e.command.op==='comment'&&e.command.req===reqId&&e.command.id===id);
    item.append(E('p',[event&&eventDate(event.recorded_at),personLabel(c.actor),kinds[c.kind]||'Comment',c.withdrawn&&'Withdrawn'].filter(Boolean).join(' · '),'hint'));
    if(c.reply_to)cardFact(item,'In reply to',q.comments[c.reply_to]?.text);
    if(c.ac)cardFact(item,'Acceptance criterion',acTitle(r,c.ac));
    item.append(E('p',c.text));d.append(item);
  }
}
if (window.SNAPSHOT) {
  document.body.classList.remove('checking-session');
  document.body.classList.add('snapshot');
  $('login-screen').remove();
  $('refresh').remove();
  document.body.classList.add('authenticated');
  data = window.SNAPSHOT;
  ws = data.workspace;
  const option=E('option',data.state.title);option.value=ws;$('workspaces').replaceChildren(option);
  render();
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
function openRequirement(id) {
  filters={domain:'',status:'',person:''};render();
  const req=[...$('tree').querySelectorAll('.tree-requirement')].find(e=>e.dataset.requirement===id);
  if(!req)return;
  for(let node=req;node;node=node.parentElement)if(node.tagName==='DETAILS')node.open=true;
  req.scrollIntoView({block:'start'});
}
function intake(root) {
  const entries=Object.entries(data.state.intake);
  if(!entries.length)return;
  const section=details(root,'Intake · '+entries.length);
  for(const [id,item] of entries){
    const entry=details(section,id+' · '+item.author);entry.append(E('p',item.said));
    const intakeActions=E('span',undefined,'intake-actions');
    intakeActions.append(notesButton('intake',id,item.said));
    const intakeFiles=filesButton('intake',id,item.said);
    if(intakeFiles)intakeActions.append(intakeFiles);
    entry.querySelector('summary').append(intakeActions);

    for(const [pid,point] of Object.entries(item.points)){
      const block=details(entry,pid+' · '+(point.outcome||'Unreviewed'));
      block.append(E('p',point.said));
      const pointActions=E('span',undefined,'intake-actions');
      pointActions.append(notesButton('point',pid,point.said,id));
      const pointFiles=filesButton('point',pid,point.said,id);
      if(pointFiles)pointActions.append(pointFiles);
      block.querySelector('summary').append(pointActions);

      for(const [key,value] of Object.entries(point.answers||{}))block.append(E('p',key+': '+value));
      if(point.reason)block.append(E('p',point.reason));
      if(point.req)block.append(button(point.req,()=>openRequirement(point.req),'node-link'));
    }
  }
}
function copyButton(kind,id,title) {
  const b=button('',async()=>{
    const systemId=data.system_ids?.[kind]?.[id];
    if(!systemId)throw Error('System ID unavailable. Refresh the page.');
    const text=systemId;
    try{
      if(!navigator.clipboard?.writeText)throw Error('Clipboard unavailable');
      await navigator.clipboard.writeText(text);
      notify('Copied ID for your agent.');
    }catch{
      const dialog=E('dialog',undefined,'copy-dialog');
      dialog.setAttribute('aria-label','Copy for your agent');
      const area=E('input');area.readOnly=true;area.value=text;area.setAttribute('aria-label','ID');
      dialog.append(E('h2','Copy for your agent'),E('p','Automatic copying is unavailable. Select the text and copy it with ⌘C or Ctrl+C.'),area,button('Close',()=>dialog.close()));
      dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();area.focus();area.select();
    }
  },'copy-context');
  const svgNS='http://www.w3.org/2000/svg';
  const icon=document.createElementNS(svgNS,'svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'14',height:'14',fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true',focusable:'false'}))icon.setAttribute(key,value);
  const square=document.createElementNS(svgNS,'rect');
  for(const [key,value] of Object.entries({x:'8',y:'8',width:'12',height:'12',rx:'2'}))square.setAttribute(key,value);
  const back=document.createElementNS(svgNS,'path');back.setAttribute('d','M16 8V4a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h4');
  icon.append(square,back);
  const face=icon.cloneNode(false);face.setAttribute('width','18');face.setAttribute('height','18');face.classList.add('agent-face');
  for(const [tag,attrs] of [
    ['rect',{x:'4',y:'7',width:'16',height:'13',rx:'4'}],
    ['path',{d:'M12 7V4m-2 0h4M1 12v4m22-4v4M9 16h6'}],
    ['circle',{cx:'8.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}],
    ['circle',{cx:'15.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}]
  ]){const part=document.createElementNS(svgNS,tag);for(const [key,value] of Object.entries(attrs))part.setAttribute(key,value);face.append(part);}
  b.append(icon,face);
  b.title='Copy for agent';
  b.setAttribute('aria-label','Copy '+kind+' ID: '+id);
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}

function orderedEntries(kind,parent=null) {
  const map=kind==='domain'?data.state.domains:kind==='capability'?data.state.capabilities:data.state.requirements;
  const ids=Object.keys(map).filter(id=>kind==='domain'?!map[id].archived:(kind==='capability'?map[id].domain:map[id].capability)===parent).sort();
  const key=kind+(parent?':'+parent:'');
  const saved=(data.state.tree_order?.[key]||[]).filter(id=>ids.includes(id));
  return [...saved,...ids.filter(id=>!saved.includes(id))].map(id=>[id,map[id]]);
}
function itemActions(kind,id,title,reqId=null,revision=null){
  const actions=E('div',undefined,'node-actions');
  // Номер ревизии стоит ВПЛОТНУЮ к истории: это один вопрос — «которая это
  // редакция и что было до неё». Отдельной строкой под заголовком он читался
  // как свойство требования, хотя на деле это отметка версии.
  actions.append(copyButton(kind,id,title));
  actions.append(notesButton(kind,id,title,reqId));
  const files=filesButton(kind,id,title,reqId);
  if(files)actions.append(files);
  if(revision!==null)actions.append(E('span','rev. '+revision,'rev-chip'));
  const history=button('',()=>showHistory(kind,id,title,reqId),'history-button');history.title='View history';history.setAttribute('aria-label','View '+kind+' history: '+id);
  const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'16',height:'16',fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(key,value);
  const path=document.createElementNS(icon.namespaceURI,'path');path.setAttribute('d','M3 11a9 9 0 1 1 2.7 7.4M3 4v7h7M12 7v5l3 2');icon.append(path);history.append(icon);
  history.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});actions.append(history);return actions;
}
function showHistory(kind,id,title,reqId){
  const q=data.state.requirements[kind==='requirement'?id:reqId];
  const created=data.history.find(e=>e.command.op===kind&&e.command.id===id)?.position||0;
  const events=data.history.filter(event=>{
    if(event.position<created)return false;
    const c=event.command;
    if(c.op==='reorder_tree')return c.kind===kind&&c.ids.includes(id)&&c.expected.indexOf(id)!==c.ids.indexOf(id);
    if(kind==='requirement')return c.req===id;
    if(kind!=='ac')return ((c.op===kind||c.op==='edit_'+kind||(kind==='domain'&&c.op==='set_domain_archived'))&&c.id===id)||c.op==='confirm_tree';
    if(c.req!==reqId)return false;
    if(c.op==='revise')return c.acs.some(a=>a.id===id)||!!q.revisions[c.expected_revision-1]?.acs.some(a=>a.id===id);
    const revision=q.revisions.find(r=>r.number===c.revision);
    if(!revision?.acs.some(a=>a.id===id))return false;
    if(c.ac)return c.ac===id;
    if(c.op==='task')return c.acs.includes(id);
    if(c.op==='complete_task')return revision.tasks[c.id]?.acs.includes(id);
    if(c.op==='plan')return c.checks.some(check=>check.ac===id);
    if(c.op==='result')return revision.runs[c.run]?.plan.some(check=>check.id===c.evidence.check&&check.ac===id);
    if(c.op==='resolve')return revision.issues[c.id]?.ac===id;
    // Событие попадает в историю AC, только если НАЗЫВАЕТ его. Раньше здесь
    // стояло true, и всё, что относилось к ревизии требования и не называло
    // ни одного условия, показывалось в истории КАЖДОГО его условия — история
    // AC читалась как история REQ.
    return false;
  });
  const dialog=E('dialog',undefined,'history-dialog');dialog.setAttribute('aria-label','History: '+title);
  const header=E('div',undefined,'history-dialog-header'),heading=E('div');heading.append(E('small','History'),E('h2',title));
  heading.tabIndex=-1;heading.autofocus=true;
  if(kind==='requirement'){
    const card=[...document.querySelectorAll('[data-requirement]')].find(node=>node.dataset.requirement===id);
    const body=card?.querySelector('.requirement-detail');
    const revisions=E('select',undefined,'history-revisions');
    revisions.setAttribute('aria-label','Requirement revision');
    for(const revision of [...q.revisions].reverse()){
      const option=E('option','Revision '+revision.number+(revision===q.revisions.at(-1)?' · Latest':'')+(revision.reason?' · '+revision.reason:''));
      option.value=revision.number;revisions.append(option);
    }
    revisions.value=body?.dataset.revision||q.revisions.at(-1).number;
    revisions.onchange=()=>{
      if(!body)return;
      requirementDetails(body,id,Number(revisions.value));
      card.open=true;dialog.close();
      body.scrollIntoView({block:'nearest'});
    };
    heading.append(revisions);
  }
  const close=button('',()=>dialog.close(),'history-close');close.setAttribute('aria-label','Close history');
  const closeIcon=document.createElementNS('http://www.w3.org/2000/svg','svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'18',height:'18',fill:'none',stroke:'currentColor','stroke-width':'1.8','stroke-linecap':'round','aria-hidden':'true'}))closeIcon.setAttribute(key,value);
  const closePath=document.createElementNS(closeIcon.namespaceURI,'path');closePath.setAttribute('d','M6 6l12 12M6 18L18 6');closeIcon.append(closePath);close.append(closeIcon);header.append(heading,close);dialog.append(header);
  const content=E('div',undefined,'history-timeline');content.dataset.itemId=id;content.dataset.itemKind=kind;
  for(const event of events.reverse())historyEvent(content,event,q);
  if(!content.childNodes.length)content.append(E('p','No changes recorded yet.','hint'));
  dialog.append(content);dialog.addEventListener('close',()=>dialog.remove());
  dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close();}});
  document.body.append(dialog);dialog.showModal();
}

/* ---- Заметки: пояснение, обсуждение и вложения на любом объекте ---- */

// Адрес объекта. Тот же вид, что понимает сервер, — вид:идентификатор.
function address(kind,id,reqId){return ['ac','point'].includes(kind)?kind+':'+reqId+'/'+id:kind+':'+id;}
function notesOf(target){return (data?.state?.notes||{})[target]||{description:'',comments:{},attachments:{}};}
function liveNotes(target){
  const n=notesOf(target);
  return {
    description:n.description||'',
    comments:Object.entries(n.comments||{}).filter(([,c])=>!c.withdrawn),
    attachments:Object.entries(n.attachments||{}).filter(([,a])=>!a.removed),
  };
}
// Описание объекта может жить в нём самом (домен, капабилити) — тогда
// показываем его, а не пустую заметку. Одно поле в интерфейсе, два места
// хранения: человеку разница не нужна, ему нужен текст.
function describedText(kind,id,reqId,target){
  if(kind==='domain')return data.state.domains[id]?.description||'';
  if(kind==='capability')return data.state.capabilities[id]?.description||'';
  return notesOf(target).description||'';
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
function notesButton(kind,id,title,reqId){
  const target=address(kind,id,reqId);
  const n=liveNotes(target);
  const count=n.comments.length;
  const b=button('',()=>showNotes(kind,id,title,reqId),'notes-button');
  if(count)b.classList.add('has-notes');
  b.disabled=count===0;
  b.title=count?'Comments · '+count:'No comments';
  b.setAttribute('aria-label','Notes on '+kind+' '+id+(count?': '+count:''));
  b.append(svgIcon('M21 11.5a8.4 8.4 0 0 1-9 8.4 9 9 0 0 1-3.8-.9L3 20.5l1.6-4.6A8.4 8.4 0 0 1 3.5 11 8.5 8.5 0 0 1 12 3a8.5 8.5 0 0 1 9 8.5Z'));
  if(count)b.append(E('span',String(count),'notes-count'));
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}
function filesButton(kind,id,title,reqId){
  const target=address(kind,id,reqId),attachments=liveNotes(target).attachments;
  const b=button('',()=>showFiles(title,attachments),'files-button');
  b.dataset.target=target;
  b.disabled=attachments.length===0;
  if(attachments.length)b.classList.add('has-files');
  b.title=attachments.length?'Files · '+attachments.length:'No files';
  b.setAttribute('aria-label','Files on '+kind+' '+id+': '+attachments.length);
  b.setAttribute('aria-haspopup','dialog');
  b.append(svgIcon(FILE_ICON));
  if(attachments.length)b.append(E('span',String(attachments.length),'files-count'));
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}
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
function showFiles(title,attachments){
  const dialog=E('dialog',undefined,'notes-dialog files-dialog');
  dialog.setAttribute('aria-label','Files: '+title);
  const header=E('div',undefined,'history-dialog-header'),heading=E('div');
  heading.append(E('small','Files · '+attachments.length),E('h2',title));
  const close=button('',()=>dialog.close(),'history-close');
  close.setAttribute('aria-label','Close files');close.append(svgIcon('M6 6l12 12M6 18L18 6',18));
  header.append(heading,close);dialog.append(header,attachmentList(attachments));
  dialog.addEventListener('close',()=>dialog.remove());
  dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close();}});
  document.body.append(dialog);dialog.showModal();
}
function fileSize(bytes){
  if(bytes<1024)return bytes+' B';
  if(bytes<1024*1024)return Math.round(bytes/1024)+' KB';
  return (bytes/(1024*1024)).toFixed(1)+' MB';
}
function showNotes(kind,id,title,reqId){
  const target=address(kind,id,reqId);
  const n=liveNotes(target);
  const dialog=E('dialog',undefined,'notes-dialog');dialog.setAttribute('aria-label','Notes: '+title);
  const header=E('div',undefined,'history-dialog-header'),heading=E('div');
  heading.append(E('small','Notes'),E('h2',title));
  const close=button('',()=>dialog.close(),'history-close');close.setAttribute('aria-label','Close notes');
  close.append(svgIcon('M6 6l12 12M6 18L18 6',18));header.append(heading,close);dialog.append(header);
  const body=E('div',undefined,'notes-body');dialog.append(body);

  /* Описание */
  const descSection=E('section',undefined,'notes-section');
  descSection.append(E('h3','Description','notes-heading'));
  const descText=describedText(kind,id,reqId,target);
  descSection.append(E('p',descText||'No description yet.',descText?'notes-description':'hint'));
  if(kind==='ac'){
    const own=currentAc(reqId,id)?.description;
    if(own)descSection.append(E('p','From the revision: '+own,'hint'));
  }
  body.append(descSection);

  /* Обсуждение */
  const talk=E('section',undefined,'notes-section');
  talk.append(E('h3','Discussion'+(n.comments.length?' · '+n.comments.length:''),'notes-heading'));
  const thread=E('div',undefined,'notes-thread');
  const ordered=n.comments.sort((a,b)=>String(a[1].at).localeCompare(String(b[1].at)));
  for(const [,c] of ordered){
    const item=E('article',undefined,'note');
    const meta=E('div',undefined,'note-meta');
    meta.append(E('span',c.actor,'note-actor'),E('time',when(c.at),'note-at'));
    item.append(meta,E('p',c.text,'note-text'));
    thread.append(item);
  }
  if(!ordered.length)thread.append(E('p','Nothing said here yet.','hint'));
  talk.append(thread);
  body.append(talk);

  dialog.addEventListener('close',()=>dialog.remove());
  dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)dialog.close();}});
  document.body.append(dialog);dialog.showModal();
}
function currentAc(reqId,acId){
  return data.state.requirements[reqId]?.revisions.at(-1)?.acs.find(a=>a.id===acId);
}
