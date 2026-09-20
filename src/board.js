/* SNAPSHOT */
let ws = '', data = null, viewerActor = '';
let orderSaving=false,draggedNode=null,orderMenu=null,filterMenu=null;
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
  $('workspaces').disabled = list.length === 0;
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
  if(filters.domain&&!data.state.domains[filters.domain])filters.domain='';
  if(filters.status&&!statusLabels[filters.status])filters.status='';
  const toolbar=E('section',undefined,'tree-toolbar');toolbar.setAttribute('aria-label','Filter product tree');
  const fields=E('div',undefined,'filter-fields');
  if(filters.person&&filters.person===viewerActor)filters.person='@me';
  if(filters.person&&filters.person!=='@me'&&data.state.members[filters.person]!=='human')filters.person='';
  if(filters.person==='@me'&&!viewerActor)filters.person='';
  fields.append(filterPicker('domain','Filter by domain','Domain',[
    {id:'',title:'All domains',hint:String(Object.keys(data.state.domains).length)},
    ...orderedEntries('domain').map(([id,d])=>({id,title:d.title,hint:String(Object.values(data.state.capabilities).filter(c=>c.domain===id).length)+' capabilities'}))
  ],filters.domain,value=>{filters.domain=value;applyFilters();}));
  const people=[{id:'',title:'Everyone',hint:'All responsibilities'}];
  if(viewerActor)people.push({id:'@me',title:'Assigned to me',hint:'My responsibilities'});
  for(const [id,role] of Object.entries(data.state.members))if(role==='human'&&id!==viewerActor)people.push({id,title:id,hint:'Responsibilities'});
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
  $('filter-empty').hidden=visibleDomains>0||!Object.keys(data.state.domains).length;
}
function render() {
  draggedNode?.cancel();
  closeFilterMenu();
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
    heading.append(E('span',d.title,'node-title'),E('small','DOMAIN','type-label'),itemActions('domain',did,d.title));group.append(heading);addOrdering(group,heading,'domain',null,did);
    const body=E('div',undefined,'domain-body');body.append(E('p','Decision owner: '+d.authority,'node-meta'));group.append(body);
    for(const [cid,c] of orderedEntries('capability',did)){
      const cap=E('details',undefined,'tree-capability');cap.dataset.capability=cid;cap.dataset.treeNode='capability:'+cid;cap.open=opened.has(cap.dataset.treeNode);
      const head=E('summary',undefined,'title-row');head.append(E('span',c.title,'node-title'),E('small','CAPABILITY','type-label'),itemActions('capability',cid,c.title));cap.append(head);addOrdering(cap,head,'capability',did,cid);
      const reqs=orderedEntries('requirement',cid);
      if(!reqs.length)cap.append(E('p','No requirements yet.','empty-state'));
      for(const [id,q] of reqs){
        const r=q.revisions.at(-1),tone=requirementStatus(id);
        const req=E('details',undefined,'tree-requirement '+tone);req.dataset.requirement=id;req.dataset.treeNode='requirement:'+id;req.open=opened.has(req.dataset.treeNode);
        const reqHead=E('summary',undefined,'title-row');reqHead.append(E('span',r.title,'node-title'),E('small','REQ','type-label'),E('span',statusLabels[tone],'badge '+tone),itemActions('requirement',id,r.title));
        const content=E('div',undefined,'requirement-detail');req.append(reqHead,content);addOrdering(req,reqHead,'requirement',cid,id);
        req.addEventListener('toggle',()=>{if(req.open&&!content.childNodes.length)requirementDetails(content,id);});
        if(req.open)requirementDetails(content,id);
        cap.append(req);
      }
      body.append(cap);
    }
    t.append(group);
  }
  if(!Object.keys(data.state.domains).length)t.append(E('p','No domains yet. Add the product structure through MCP.','empty-state'));
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
  if(current)root.dataset.status=requirementStatus(selected);
  if(!current)root.append(E('h3',r.title));
  root.append(E('p',selected+' · revision '+r.number+' · owner '+r.owner,'hint'),E('p',r.body));
  const revisions = E('select');
  for (const old of q.revisions) {
    const o = E('option', 'Revision ' + old.number + ' · ' + old.reason);
    o.value = old.number;
    revisions.append(o);
  }
  revisions.setAttribute('aria-label','Requirement revision');
  revisions.value = r.number;
  revisions.onchange = e => {
    requirementDetails(root,selected,+e.target.value);
  };
  root.append(revisions);
  root.append(E('p',current?statusLabels[requirementStatus(selected)]:'Previous revision. See its checks and history for evidence.',current?'badge '+requirementStatus(selected):'hint'));
  const base = {
    req: selected,
    revision: r.number
  };
  for (const a of r.acs) {
    const status = s?.acs[a.id];
    const item = E('article',undefined,'ac-card '+(status?.verification==='pass'?'pass':status?.verification==='fail'?'fail':'unverified'));
    const heading=E('div',undefined,'ac-heading');heading.append(E('h3',a.text),itemActions('ac',a.id,a.text,selected));item.append(heading);
    item.append(E('small','AC · '+a.id), E('span', status?.verification || 'See history', 'badge ' + (status?.verification || '')), E('p', status?.ready ? 'Ready to work' : a.assessment?.reason || 'Awaiting review'));
    if (a.before_launch) item.append(E('small', 'Required before launch'));
    if (a.depends_on?.length) item.append(E('p','Depends on: '+a.depends_on.join(', ')));
    if (a.decider) item.append(E('p', 'Decision: ' + a.decider));
    root.append(item);
  }
  tasks(root, r);
  runs(root, r, base);
  comments(root, q, r);
}

function personLabel(actor){
  if(actor===viewerActor)return 'You';
  return {agent:'Agent',runner:'Test runner'}[data.state.members[actor]]||actor||'Not assigned';
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
  const names={domain:'Domain created',capability:'Capability created',reorder_tree:'Order changed',edit_domain:'Domain updated',edit_capability:'Capability updated',confirm_tree:'Structure confirmed',revise:c.expected_revision?'Requirement updated':'Requirement created',comment:{decision:'Decision recorded',accept:'Suggestion accepted',change:'Changes requested',reject:'Suggestion declined',correction:'Reply corrected'}[c.kind]||'Comment added',assess:'Readiness reviewed',confirm:'Wording approved',task:'Task created',complete_task:'Task completed',plan:'Checks planned',result:'Check result recorded',finish:'Checks completed',accept:'Requirement accepted',revoke:'Acceptance withdrawn',issue:'Issue confirmed',resolve:'Issue resolved',withdraw_decision:'Decision withdrawn'};
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
function tasks(root, r) {
  const d = details(root, 'Tasks · ' + Object.keys(r.tasks).length);
  for (const [id, t] of Object.entries(r.tasks)) {
    d.append(E('p', id + ' · ' + t.title + ' · ' + t.acs.join(', ') + ' · ' + t.projects.join(', ') + ' · ' + (t.done ? 'Completed: ' + t.done : 'Assignee: ' + t.assignee)));
  }
}
function runs(root, r, base) {
  const d = details(root, 'Checks · ' + Object.keys(r.runs).length);
  for (const [id, run] of Object.entries(r.runs)) {
    const block = details(d, id + ' · ' + (run.finished ? 'Finished' : 'Running'));
    block.append(E('p', 'Subject: ' + JSON.stringify(run.subject)), E('p', 'Scope: ' + run.scope + ' · discovery ' + (run.discovery_complete ? 'complete' : 'incomplete')));
    for (const c of run.plan) {
      const result = run.results[c.id];
      const recorded = data.history.find(e => e.command.op === 'result' && e.command.req === base.req && e.command.revision === base.revision && e.command.run === id && e.command.evidence.check === c.id);
      if (recorded) block.append(E('small', recorded.actor + ' · ' + recorded.recorded_at));
      block.append(E('small','Method: '+c.method+' · AC: '+c.ac));
      if(result?.link)block.append(E('p','Evidence: '+result.link));
      block.append(E('h3', c.id), E('p', 'Expected: ' + c.expected.join(', ')), E('p', result ? 'Observed: ' + result.observed.join(', ') + ' · ' + result.outcome + ' · ' + result.note : 'No result yet'));
    }
  }
}
function comments(root, q, r) {
  const d = details(root, 'Discussion');
  d.open = true;
  for (const [id, c] of Object.entries(q.comments).filter(([, c]) => c.revision === r.number)) {
    d.append(E('small', id + ' · ' + c.actor + ' · ' + c.kind + (c.withdrawn ? ' · withdrawn' : '') + (c.reply_to ? ' · reply to ' + c.reply_to : '')), E('p', c.text));
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
    for(const [pid,point] of Object.entries(item.points)){
      const block=details(entry,pid+' · '+(point.outcome||'Unreviewed'));
      block.append(E('p',point.said));
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
    const text=systemId+' — '+title;
    try{
      if(!navigator.clipboard?.writeText)throw Error('Clipboard unavailable');
      await navigator.clipboard.writeText(text);
      notify('Copied for your agent.');
    }catch{
      const dialog=E('dialog',undefined,'copy-dialog');
      dialog.setAttribute('aria-label','Copy for your agent');
      const area=E('input');area.readOnly=true;area.value=text;area.setAttribute('aria-label','ID and title');
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
  b.setAttribute('aria-label','Copy '+kind+' ID and title: '+id);
  b.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  return b;
}

function orderedEntries(kind,parent=null) {
  const map=kind==='domain'?data.state.domains:kind==='capability'?data.state.capabilities:data.state.requirements;
  const ids=Object.keys(map).filter(id=>kind==='domain'||(kind==='capability'?map[id].domain:map[id].capability)===parent).sort();
  const key=kind+(parent?':'+parent:'');
  const saved=(data.state.tree_order?.[key]||[]).filter(id=>ids.includes(id));
  return [...saved,...ids.filter(id=>!saved.includes(id))].map(id=>[id,map[id]]);
}
function itemActions(kind,id,title,reqId=null){
  const actions=E('div',undefined,'node-actions');actions.append(copyButton(kind,id,title));
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
    if(kind!=='ac')return ((c.op===kind||c.op==='edit_'+kind)&&c.id===id)||c.op==='confirm_tree';
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
    return true;
  });
  const dialog=E('dialog',undefined,'history-dialog');dialog.setAttribute('aria-label','History: '+title);
  const header=E('div',undefined,'history-dialog-header'),heading=E('div');heading.append(E('small','History'),E('h2',title));
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

function closeOrderMenu(){orderMenu?.remove();orderMenu=null;}
document.addEventListener('click',e=>{if(orderMenu&&!orderMenu.contains(e.target))closeOrderMenu();});
document.addEventListener('keydown',e=>{if(e.key==='Escape')closeOrderMenu();});
async function saveOrder(kind,parent,expected,ids,focusId) {
  if(orderSaving||ids.every((id,index)=>id===expected[index]))return;
  const targetWorkspace=ws;orderSaving=true;closeOrderMenu();
  document.querySelectorAll('.reorder-handle').forEach(b=>b.disabled=true);
  try{
    const response=await fetch('/api/workspaces/'+encodeURIComponent(targetWorkspace)+'/order',{
      method:'POST',headers:{'Content-Type':'application/json'},
      body:JSON.stringify({request_id:crypto.randomUUID(),kind,parent,expected,ids})
    });
    const result=await response.json();
    if(!response.ok){if(ws===targetWorkspace)await load();throw Error(result.error||'Could not save the order.');}
    if(ws===targetWorkspace){
      await load();
    }
    notify('Order saved.');
  }finally{
    orderSaving=false;document.querySelectorAll('.reorder-handle').forEach(b=>b.disabled=false);
    if(ws===targetWorkspace){
      const moved=[...document.querySelectorAll('[data-tree-node]')].find(e=>e.dataset.treeNode===kind+':'+focusId);
      moved?.querySelector(':scope > summary > .reorder-handle')?.focus({preventScroll:true});
    }
  }
}
function moveSibling(kind,parent,id,offset) {
  const expected=orderedEntries(kind,parent).map(([id])=>id),ids=[...expected],index=ids.indexOf(id),next=index+offset;
  if(next<0||next>=ids.length)return;
  [ids[index],ids[next]]=[ids[next],ids[index]];
  return saveOrder(kind,parent,expected,ids,id);
}
function addOrdering(node,summary,kind,parent,id) {
  if(window.SNAPSHOT||viewerActor!==data.state.admin)return;
  const handle=E('button',undefined,'reorder-handle');
  const grip=document.createElementNS('http://www.w3.org/2000/svg','svg');grip.setAttribute('viewBox','0 0 16 20');grip.setAttribute('width','16');grip.setAttribute('height','20');grip.setAttribute('aria-hidden','true');
  for(const x of [5,11])for(const y of [4,10,16]){const dot=document.createElementNS(grip.namespaceURI,'circle');dot.setAttribute('cx',x);dot.setAttribute('cy',y);dot.setAttribute('r','1.4');dot.setAttribute('fill','currentColor');grip.append(dot);}handle.append(grip);handle.type='button';handle.disabled=orderSaving;
  let ignoreClick=false;
  handle.setAttribute('aria-label','Reorder '+kind+': '+id);handle.setAttribute('aria-haspopup','menu');
  handle.title='Drag to reorder, or click for move options';summary.classList.add('can-reorder');summary.append(handle);
  handle.onclick=e=>{
    e.preventDefault();e.stopPropagation();if(ignoreClick)return;closeOrderMenu();
    const menu=E('div',undefined,'reorder-menu');menu.setAttribute('role','menu');menu.setAttribute('aria-label','Move item');
    const siblings=orderedEntries(kind,parent),index=siblings.findIndex(([key])=>key===id);
    for(const [label,offset] of [['Move up',-1],['Move down',1]]){
      const b=button(label,()=>moveSibling(kind,parent,id,offset));b.setAttribute('role','menuitem');b.disabled=index+offset<0||index+offset>=siblings.length;menu.append(b);
    }
    orderMenu=menu;document.body.append(menu);
    const rect=handle.getBoundingClientRect();menu.style.left=Math.max(8,Math.min(rect.right-150,innerWidth-158))+'px';menu.style.top=Math.max(8,Math.min(rect.bottom+5,innerHeight-100))+'px';
    menu.querySelector('button:not(:disabled)')?.focus();
  };
  handle.onkeydown=e=>{if(['ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();e.stopPropagation();Promise.resolve(moveSibling(kind,parent,id,e.key==='ArrowUp'?-1:1)).catch(notify);}};
  handle.onpointerdown=e=>{
    if(e.button!==0||orderSaving||matchMedia('(max-width:760px)').matches)return;
    const container=node.parentElement,originalNext=node.nextSibling,wasOpen=node.open;
    const expected=orderedEntries(kind,parent).map(([key])=>key),workspace=ws;
    const siblings=()=>[...container.children].filter(n=>n.dataset.treeNode?.startsWith(kind+':'));
    let active=false,preview=null,frame=null,point={x:e.clientX,y:e.clientY},valid=false;
    const start={...point},pointer=e.pointerId;
    function cleanup(commit=false){
      const moved=active;active=false;
      for(const n of siblings())for(const animation of n.getAnimations())animation.cancel();
      document.removeEventListener('pointermove',move);document.removeEventListener('pointerup',up);
      document.removeEventListener('pointercancel',cancel);document.removeEventListener('keydown',escape);
      window.removeEventListener('blur',cancel);window.removeEventListener('resize',cancel);
      cancelAnimationFrame(frame);preview?.remove();node.classList.remove('drag-source');document.body.classList.remove('tree-dragging');draggedNode=null;
      if(!moved)return;
      ignoreClick=true;setTimeout(()=>{ignoreClick=false;},0);
      node.open=wasOpen;
      if(!commit){container.insertBefore(node,originalNext?.parentElement===container?originalNext:null);return;}
      const ids=siblings().map(n=>n.dataset.treeNode.slice(kind.length+1));
      saveOrder(kind,parent,expected,ids,id).catch(notify);
    }
    function cancel(){cleanup(false);}
    function escape(event){if(event.key==='Escape'){event.preventDefault();cancel();}}
    function place(){
      if(!node.isConnected||ws!==workspace){cancel();return;}
      const bounds=container.getBoundingClientRect();
      valid=point.x>=bounds.left-16&&point.x<=bounds.right+16&&point.y>=bounds.top-40&&point.y<=bounds.bottom+40;
      preview.classList.toggle('invalid-drop',!valid);
      const x=Math.max(8,Math.min(innerWidth-preview.offsetWidth-8,point.x-preview.offsetWidth+35));
      preview.style.transform=`translate3d(${x}px,${point.y-28}px,0)`;
      if(!valid)return;
      const others=siblings().filter(n=>n!==node&&!n.hidden);
      for(const n of others)for(const animation of n.getAnimations())animation.cancel();
      const before=others.find(n=>{const r=n.getBoundingClientRect();return point.y<r.top+r.height/2;});
      const next=before||(others.at(-1)?.nextSibling);
      if(next===node||node.nextSibling===next)return;
      const positions=new Map(others.map(n=>[n,n.getBoundingClientRect().top]));
      container.insertBefore(node,next||null);
      if(!matchMedia('(prefers-reduced-motion:reduce)').matches)for(const n of others){
        const delta=positions.get(n)-n.getBoundingClientRect().top;
        if(delta)n.animate([{transform:`translateY(${delta}px)`},{transform:'translateY(0)'}],{duration:140,easing:'ease-out'});
      }
    }
    function scroll(){
      if(!active)return;
      const speed=point.y<100?-Math.min(14,(100-point.y)/4):point.y>innerHeight-70?Math.min(14,(point.y-innerHeight+70)/4):0;
      if(speed){window.scrollBy(0,speed);place();}
      if(active)frame=requestAnimationFrame(scroll);
    }
    function move(event){
      if(event.pointerId!==pointer)return;
      point={x:event.clientX,y:event.clientY};
      if(!active){
        if(Math.hypot(point.x-start.x,point.y-start.y)<5)return;
        active=true;closeOrderMenu();closeFilterMenu();node.open=false;
        preview=E('div',undefined,'drag-preview '+kind);preview.append(grip.cloneNode(true),E('div',undefined,'drag-preview-text'));
        preview.lastChild.append(E('small',kind==='requirement'?'Requirement':kind[0].toUpperCase()+kind.slice(1)),E('strong',summary.querySelector('.node-title').textContent));
        document.body.append(preview);node.classList.add('drag-source');document.body.classList.add('tree-dragging');frame=requestAnimationFrame(scroll);
      }
      event.preventDefault();place();
    }
    function up(event){if(event.pointerId===pointer)cleanup(active&&valid);}
    draggedNode={cancel};
    document.addEventListener('pointermove',move,{passive:false});document.addEventListener('pointerup',up);
    document.addEventListener('pointercancel',cancel);document.addEventListener('keydown',escape);
    window.addEventListener('blur',cancel);window.addEventListener('resize',cancel);
  };
}
