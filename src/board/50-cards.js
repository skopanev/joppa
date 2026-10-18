function commonObjectCard(item,loadedDetail,parentTitle){
  const card=E('details',undefined,'common-object');card.dataset.objectAddress=item.address;card.dataset.kind=item.kind;card.dataset.objectTitle=item.title;applyObjectColor(card,item.kind);
  const summary=E('summary',undefined,'title-row');
  const tone=item.review_required?'purple':item.work_ready?'grey':'yellow';
  const signal=item.historical?{tone:'grey',label:'Saved revision',at:item.at}:item.present===false?{tone:'grey',label:'Historical object',at:item.at}:item.signal?.tone?item.signal:{tone,label:item.review_required?'Dependency changed':item.work_ready?'Current document':'Dependency not ready',at:item.at};
  const repeatsParent=item.title===parentTitle;
  const heading=repeatsParent?data.objectTypes[item.kind].title+' · '+signal.label:item.title;
  const preview=objectTitlePreview(heading,item.kind);
  const title=nodeTitle(preview,signal),counts=item.verification_summary;title.dataset.truncated=String(!repeatsParent&&preview!==item.title);
  const isCriterion=data.objectTypes?.[item.kind]?.role==='ac';
  // The count sits on the right of the row, after the type label, not inside the title.
  let chip=null;
  if(isCriterion&&signal.tone==='green'){
    chip=E('span','Passed','verification-chip has-passes');
  }else if(!isCriterion&&counts&&verificationChildTitle(item.kind)){
    chip=E('span',undefined,'verification-chip');chip.dataset.address=item.address;chip.dataset.total=counts.total;chip.dataset.passed=counts.passed;chip.dataset.failed=counts.failed||0;chip.dataset.childTitle=verificationChildTitle(item.kind);showTestCount(chip);
  }
  // Waiting work names the first dependency that is not green yet.
  // A waiting row shows only how many dependencies are not green; the dependency list names them.
  const deps=item.waits_for?.count||1;
  const waits=item.waits_for?[E('span',deps+(deps===1?' dep':' deps'),'waits-for')]:[];
  if(waits.length)waits[0].title='Waits for '+(item.waits_for.title||item.waits_for.address);
  // Every live node shows its priority read-only in the row.
  const priority=item.process?.priority&&!item.historical&&item.present!==false?priorityBadge(item):null;
  // An empty slot keeps the count column aligned on rows without a count.
  summary.append(title,...(priority?[priority]:[]),objectTypeLabel(item.kind),...waits,chip||E('span',undefined,'count-slot'));
  // The count can colour the row dot only once both sit in the summary.
  if(chip?.dataset.total)showTestCount(chip);
  if(inputQueue(item.kind))summary.append(E('span',item.note_status==='processed'?'Processed':'Unprocessed','note-state'));
  const actions=E('span',undefined,'item-actions');
  const copy=copyButton(item.item_id||item.address);
  const history=button('',()=>showObjectRevisions(item.address),'history-button');history.type='button';
  history.title='Revisions';history.setAttribute('aria-label','Revisions');history.append(historyIcon());
  history.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  // The properties open and close from the row. The object stays open or closed.
  const fieldsToggle=button('',()=>{
    const shown=fieldsToggle.getAttribute('aria-expanded')!=='true';fieldsToggle.setAttribute('aria-expanded',String(shown));
    const fields=body.querySelector(':scope > .object-secondary > .object-details');if(fields)resizeBranch(card,()=>{fields.hidden=!shown;});
  },'fields-button');
  fieldsToggle.type='button';fieldsToggle.title='Properties';fieldsToggle.setAttribute('aria-label','Properties');fieldsToggle.append(propertiesIcon());
  fieldsToggle.setAttribute('aria-expanded','false');fieldsToggle.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  actions.append(fieldsToggle,copy,history);summary.append(actions);card.append(summary);
  const body=E('div',undefined,'object-body');card.append(body);enableBranchMotion(card);let loaded=false;
  // Fill the body with the document and its children. Both requests run at once.
  const fill=async()=>{
    const generation=loadNumber,childParams={limit:'50',parent:item.address};
    const [detail,kids]=await Promise.all([loadedDetail||objectPart({address:item.address}),objectPart(childParams).catch(()=>null)]);
    if(generation!==loadNumber||!card.isConnected)return;
    body.replaceChildren();fillCommonObject(body,detail);
    const children=E('div',undefined,'object-children');body.insertBefore(children,body.querySelector(':scope > .object-secondary'));
    if(!detail.historical){if(kids)appendObjectPage(children,kids,childParams,generation);else await appendObjectChildren(children,item.address);}
  };
  card.branchPrepare=async()=>{if(loaded)return;loaded=true;try{await fill();}catch(e){loaded=false;body.replaceChildren();notify(e);throw e;}};
  card.addEventListener('toggle',async()=>{if(card.open)syncObjectTitles(body);if(!card.open||loaded)return;loaded=true;card.branchReady=true;body.append(E('p','Loading object…','object-loading hint'));try{const before=card.getBoundingClientRect().height;await fill();stopBranchAnimation(card);animateBranchHeight(card,before,card.getBoundingClientRect().height);}catch(e){loaded=false;body.replaceChildren();notify(e);}});
  return card;
}
function syncObjectTitles(root=document){
  for(const title of root.querySelectorAll('.object-full-title')){
    const container=title.closest('.common-object,.notes-dialog');
    const heading=container?.querySelector(':scope > summary > .node-title,.history-dialog-header h2');
    title.hidden=!heading||(heading.dataset.truncated!=='true'&&heading.scrollHeight<=heading.clientHeight+1);
  }
}
window.addEventListener('resize',()=>syncObjectTitles());
function objectSection(title,cls){
  const group=E('details',undefined,'object-section '+cls);
  group.append(E('summary',title));enableBranchMotion(group);return group;
}
// The schema selects the next types that can contain verification criteria.
function verificationChildTitle(kind){
  const types=data.objectTypes,branches=new Set(Object.keys(types).filter(k=>types[k].role==='ac'));
  let changed=true;
  while(changed){const size=branches.size;for(const child of [...branches])for(const parent of types[child]?.parents||[])if(parent==='*')Object.keys(types).forEach(k=>branches.add(k));else branches.add(parent);changed=branches.size!==size;}
  return [...new Set(Object.entries(types).filter(([k,t])=>branches.has(k)&&(t.parents||[]).some(p=>p===kind||p==='*')).map(([,t])=>t.title))].join(' / ');
}
function showTestCount(chip){
  const green=Number(chip.dataset.passed),red=Number(chip.dataset.failed),total=Number(chip.dataset.total),title=chip.dataset.childTitle;
  chip.textContent=(green?green+' / '+total:total)+' '+title;
  chip.classList.toggle('has-passes',green>0);chip.classList.toggle('has-failures',red>0);
  const label=green+' of '+total+' '+title+' ready'+(red?', '+red+' failed':'');
  chip.setAttribute('aria-label',label);
  // The dot keeps the server tone of the type's status rule and the process label.
}
// A criterion takes its dot from its tests: green when every test passes,
// red when one fails or is missing, grey otherwise, and grey "No tests" without tests.
const testTone=status=>status==='pass'||status==='green'?'green':['fail','missing','red'].includes(status)?'red':'grey';
function testSummary(s){
  const passed=s.tests.filter(t=>t.status==='pass').length;
  return 'Tests: '+passed+' of '+s.tests.length+' pass'+(s.commit?' · commit '+s.commit.slice(0,7):'')+(s.commits_behind?' · '+s.commits_behind+' commits behind':'')+(s.tests_draft?' · list needs saving':'');
}
async function applyTestStatuses(items){
  const criteria=items.filter(item=>data.objectTypes?.[item.kind]?.role==='ac').map(item=>item.address);
  if(!criteria.length)return;
  const ready=new Set(items.filter(item=>item.signal?.label==='Ready for work').map(item=>item.address));
  let reply;
  try{reply=await api('workspaces/'+encodeURIComponent(ws)+'/tests?ac='+encodeURIComponent(criteria.join(',')));}catch{return;}
  for(const status of reply.items||[reply]){
    for(const card of document.querySelectorAll('.common-object[data-object-address="'+CSS.escape(status.address)+'"]')){
      const dot=card.querySelector(':scope > summary .status-circle');
      // A criterion without tests is grey "No tests", so yellow on the board means mixed only.
      // A ready criterion with grey tests keeps "Ready for work" before its test status.
      const label=(ready.has(status.address)&&testTone(status.status)==='grey'?'Ready for work · ':'')+(status.reason==='no tests'?'No tests':testSummary(status));
      if(dot)dot.replaceWith(statusCircle({tone:testTone(status.status),label}));
    }
  }
}
// The tests that prove a criterion, each with its latest result.
function criterionTestsSection(address){
  const group=objectSection('Tests','object-tests');
  api('workspaces/'+encodeURIComponent(ws)+'/tests?ac='+encodeURIComponent(address)).then(s=>{
    group.querySelector(':scope > summary').textContent='Tests '+s.tests.length;
    if(!s.tests.length){group.append(E('p','No tests are listed for this criterion.','hint'));return;}
    group.append(E('p',testSummary(s),'node-meta'));
    if(s.tests_draft)group.append(E('p','The criterion text changed. Save the test list again.','dependency-review'));
    for(const t of s.tests){
      const row=E('div',undefined,'ac-check');
      const head=E('p',undefined,'ac-check-heading');
      head.append(statusCircle({tone:testTone(t.status),label:t.status.replace('_',' ')}),E('span',t.address));
      row.append(head,E('p',[t.status.replace('_',' '),t.commit&&'commit '+t.commit.slice(0,7)].filter(Boolean).join(' · '),'detail-fact'));
      if(t.message)row.append(E('pre',t.message));
      group.append(row);
    }
  }).catch(e=>group.append(E('p',e.message,'hint')));
  return group;
}
function fillCommonObject(body,detail){
  body.dataset.objectContextAddress=detail.address;
  const r=detail.document,d=r.data;
  const historical=detail.historical??r.revision!==detail.current_revision;
  const heading=body.parentElement?.querySelector(':scope > summary > .node-title,.history-dialog-header h2');
  const full=heading?E('div',undefined,'object-field object-full-title'):null;
  if(full){full.hidden=heading.dataset.truncated!=='true'&&heading.scrollHeight<=heading.clientHeight+1;full.append(E('small','Full title','type-label'),E('p',d.title,'node-description'));}
  if(historical)body.append(E('p','Saved document revision. Current status, comments, files and incoming links are not shown.','hint'));
  if(r.present===false)body.append(E('p','Historical object. This object is absent from the current tree.','dependency-review'));
  const secondary=E('div',undefined,'object-secondary');
  // A view without the row Fields button, such as the revision dialog, always shows the fields.
  const fields=E('div',undefined,'object-details'),fieldsToggle=body.parentElement?.querySelector(':scope > summary .fields-button');
  fields.hidden=!!fieldsToggle&&fieldsToggle.getAttribute('aria-expanded')!=='true';
  const meta=E('div',undefined,'object-meta');
  meta.append(E('span','Revision '+r.revision),E('span','Owner: '+personLabel(d.owner||'Unassigned')));fields.append(meta);
  if(full)fields.append(full);
  const verification=detail.process?.verification&&({pass:'Passed',fail:'Failed',unverified:'Unverified'}[detail.process.verification]||detail.process.verification);
  if(verification)fields.append(E('p','Verification: '+verification,'node-meta'));
  const status=typeof detail.process?.status==='string'?detail.process.status:detail.signal?.label;
  if(detail.process?.status&&!inputQueue(detail.kind)&&status!==verification&&status!=='Checks '+verification?.toLowerCase())fields.append(E('p','Status: '+status,'node-meta'));
  const assessment=detail.process?.readiness_blocker||detail.process?.assessment;
  if(!historical&&assessment)fields.append(E('p',assessmentHint(assessment).trim(),'node-meta'));
  if(detail.review_required)body.append(E('p','A dependency changed. Review the linked revision.','dependency-review'));
  const contentKey=['body','description','contract'].find(key=>typeof d[key]==='string'&&d[key].trim());
  if(contentKey)body.append(markdownBlock(d[contentKey],'object-content node-description'));
  for(const [key,value] of Object.entries(d)){
    if(['title','owner',contentKey].includes(key)||value===null||value===''||(Array.isArray(value)&&value.length===0))continue;
    const field=E('div',undefined,'object-field');
    const refRule=data.objectTypes?.[detail.kind]?.references?.[key];
    field.append(E('small',refRule?refRule.title:key.replaceAll('_',' '),'type-label'));
    if(refRule){
      const vals=Array.isArray(value)?value:[value];
      const refs=E('div',undefined,'object-links reference-links');
      for(const v of vals){
        const targetAddress=v.includes(':')?v:refRule.target+':'+v;
        const title=detail.references?.find(l=>l.target===targetAddress)?.title||detail.links?.find(l=>l.link.target===targetAddress)?.title||v;
        refs.append(objectLink(String(title),targetAddress));
      }
      field.append(refs);
    }else{
      field.append(E('p',typeof value==='string'?value:JSON.stringify(value),'node-description'));
    }
    fields.append(field);
  }
  secondary.append(fields);
  // Blocking dependencies are shown read-only on the board.
  const deps=!historical&&r.present!==false&&body.parentElement?.classList.contains('common-object')?dependencyList(detail):null;
  if(deps)secondary.append(deps);
  if(!historical&&data.objectTypes?.[detail.kind]?.role==='ac')secondary.append(criterionTestsSection(detail.address));
  const links=new Map();
  for(const {link,title,current_revision,present} of detail.links||[]){const key=link.target+'@'+(link.revision||'');const label='Links to · '+link.relation+(link.revision?' · revision '+link.revision:'')+(link.revision&&current_revision!==link.revision&&detail.review_required===true?' · changed':'')+(present===false?' · historical object':'')+(historical?' · current title':'');const entry=links.get(key)||{address:link.target,title:title||link.target,revision:link.revision,labels:[]};entry.labels.push(label);links.set(key,entry);}
  for(const link of detail.incoming||[]){const key=link.source+'@';const entry=links.get(key)||{address:link.source,title:link.title||link.source,labels:[]};entry.labels.push('Linked from · '+link.relation);links.set(key,entry);}
  if(links.size){
    const group=objectSection('Links '+links.size,'object-relations');
    const rows=E('div',undefined,'object-links');
    for(const entry of links.values()){const row=objectLink(entry.title,entry.address,entry.revision);row.append(E('small',[...new Set(entry.labels)].join(' · '),'object-link-meta'));rows.append(row);}
    group.append(rows);secondary.append(group);
  }
  const annotations=historical?{}:detail.annotations||{};
  if(annotations.description){const context=E('div',undefined,'object-field');context.append(E('small','Context','type-label'),E('p',annotations.description,'node-description'));fields.append(context);}
  const comments=Object.values(annotations.comments||{}).filter(comment=>!comment.withdrawn);
  if(comments.length){
    const group=objectSection(comments.length+' '+(comments.length===1?'comment':'comments'),'object-comments');
    for(const comment of comments){const row=E('article',undefined,'note');row.append(E('p',personLabel(comment.actor)+' · '+when(comment.at),'note-meta'),markdownBlock(comment.text,'note-text'));group.append(row);}
    secondary.append(group);
  }
  const files=historical?[]:Array.isArray(detail.files)?detail.files.map(file=>[file.id,file]):Object.entries(annotations.attachments||{}).filter(([,file])=>!file.removed);if(files.length){const group=objectSection('Files '+files.length,'object-files');group.append(attachmentList(files));secondary.append(group);}
  body.append(secondary);
}
// Blocking dependencies of a node, shown read-only on the board.
function dependencyList(detail){
  const blocking=(detail.links||[]).filter(({link})=>link.relation==='depends_on'&&link.blocking);
  if(!blocking.length)return null;
  const box=E('div',undefined,'object-dependencies');
  for(const {link,title} of blocking){
    const name=title||link.target,row=E('p',undefined,'dependency-row');
    row.append(E('span','Depends on','hint'),objectLink(name,link.target));box.append(row);
  }
  return box;
}
async function showObjectRevisions(address){
  const generation=loadNumber;
  const detail=await objectPart({address});if(generation!==loadNumber)return;
  const dialog=E('dialog',undefined,'notes-dialog');dialog.setAttribute('aria-label',detail.document.data.title);
  applyObjectColor(dialog,detail.kind);
  const header=E('div',undefined,'history-dialog-header');
  const heading=E('div'),title=E('h2');
  const setTitle=record=>{const text=record.document.data.title;title.textContent=objectTitlePreview(text,record.kind);title.dataset.truncated=String(title.textContent!==text);};
  setTitle(detail);heading.append(title);header.append(heading);
  const close=button('Close',()=>dialog.close(),'history-close');header.append(close);dialog.append(header);
  const body=E('div',undefined,'notes-body');dialog.append(body);fillCommonObject(body,detail);
  const revisions=E('label',undefined,'dialog-revisions');revisions.append(E('span','Object revision'));const select=E('select');select.setAttribute('aria-label','Object revision');for(let n=detail.current_revision;n>0;n--){const option=E('option','Revision '+n);option.value=n;select.append(option);}revisions.append(select);header.append(revisions);let request=0;select.onchange=async()=>{const mine=++request;try{const old=await objectPart({address,revision:select.value});if(mine!==request||!dialog.isConnected)return;setTitle(old);body.replaceChildren();fillCommonObject(body,old);}catch(e){notify(e);}};
  dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();
}
