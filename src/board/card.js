// The object row and its lazy body.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {historyIcon,propertiesIcon} from 'board/icons.js';
import {notify,button} from 'board/feedback.js';
import {copyButton} from 'board/clipboard.js';
import {objectPart} from 'board/api.js';
import {stopBranchAnimation,animateBranchHeight,enableBranchMotion,resizeBranch} from 'board/motion.js';
import {nodeTitle} from 'board/status.js';
import {valueSignal} from 'board/result-filter.js';
import {applyObjectColor,objectTitlePreview,objectTypeLabel,inputQueue,verificationChildTitle} from 'board/object-types.js';
import {appendObjectChildren,appendObjectPage} from 'board/object-list.js';
import {fillCommonObject} from 'board/object-detail.js';
import {showObjectRevisions} from 'board/revisions.js';
export function commonObjectCard(item,loadedDetail,parentTitle){
  const card=E('details',undefined,'common-object');card.dataset.objectAddress=item.address;card.dataset.kind=item.kind;card.dataset.objectTitle=item.title;applyObjectColor(card,item.kind);
  const summary=E('summary',undefined,'title-row');
  const tone=item.review_required?'purple':item.work_ready?'grey':'yellow';
  const signal=item.historical?{tone:'grey',label:'Saved revision',at:item.at}:item.present===false?{tone:'grey',label:'Historical object',at:item.at}:item.signal?.tone?valueSignal(item):{tone,label:item.review_required?'Dependency changed':item.work_ready?'Current document':'Dependency not ready',at:item.at};
  const repeatsParent=item.title===parentTitle;
  const heading=repeatsParent?state.data.objectTypes[item.kind].title+' · '+signal.label:item.title;
  const preview=objectTitlePreview(heading,item.kind);
  const title=nodeTitle(preview,signal),counts=item.verification_summary;title.dataset.truncated=String(!repeatsParent&&preview!==item.title);
  const isCriterion=state.data.objectTypes?.[item.kind]?.role==='ac';
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
  // The properties open and close from the row. A closed object opens with its properties shown.
  const fieldsToggle=button('',()=>{
    const shown=!card.open||fieldsToggle.getAttribute('aria-expanded')!=='true';fieldsToggle.setAttribute('aria-expanded',String(shown));
    const fields=body.querySelector(':scope > .object-details');
    if(!card.open){if(fields)fields.hidden=false;card.open=true;return;}
    if(fields)resizeBranch(card,()=>{fields.hidden=!shown;});
  },'fields-button');
  fieldsToggle.type='button';fieldsToggle.title='Properties';fieldsToggle.setAttribute('aria-label','Properties');fieldsToggle.append(propertiesIcon());
  fieldsToggle.setAttribute('aria-expanded','false');fieldsToggle.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();});
  actions.append(fieldsToggle,copy,history);summary.append(actions);card.append(summary);
  const body=E('div',undefined,'object-body');card.append(body);enableBranchMotion(card);let loaded=false;
  // Fill the body with the document and its children. Both requests run at once.
  const fill=async()=>{
    const generation=state.loadNumber,childParams={limit:'50',parent:item.address};
    const [detail,kids]=await Promise.all([loadedDetail||objectPart({address:item.address}),objectPart(childParams).catch(()=>null)]);
    if(generation!==state.loadNumber||!card.isConnected)return;
    body.replaceChildren();fillCommonObject(body,detail);
    const children=E('div',undefined,'object-children');body.insertBefore(children,body.querySelector(':scope > .object-secondary'));
    if(!detail.historical){if(kids)appendObjectPage(children,kids,childParams,generation);else await appendObjectChildren(children,item.address);}
  };
  card.branchPrepare=async()=>{if(loaded)return;loaded=true;try{await fill();}catch(e){loaded=false;body.replaceChildren();notify(e);throw e;}};
  card.addEventListener('toggle',async()=>{if(card.open)syncObjectTitles(body);if(!card.open||loaded)return;loaded=true;card.branchReady=true;body.append(E('p','Loading object…','object-loading hint'));try{const before=card.getBoundingClientRect().height;await fill();stopBranchAnimation(card);animateBranchHeight(card,before,card.getBoundingClientRect().height);}catch(e){loaded=false;body.replaceChildren();notify(e);}});
  return card;
}
// Every live node shows its priority read-only in the row.
function priorityBadge(item){
  const badge=E('span',item.process.priority.toUpperCase(),'priority-badge');
  badge.setAttribute('aria-label','Priority of '+item.title);
  return badge;
}
function showTestCount(chip){
  const green=Number(chip.dataset.passed),red=Number(chip.dataset.failed),total=Number(chip.dataset.total),title=chip.dataset.childTitle;
  chip.textContent=(green?green+' / '+total:total)+' '+title;
  chip.classList.toggle('has-passes',green>0);chip.classList.toggle('has-failures',red>0);
  const label=green+' of '+total+' '+title+' ready'+(red?', '+red+' failed':'');
  chip.setAttribute('aria-label',label);
  // The dot keeps the server tone of the type's status rule and the process label.
}
function syncObjectTitles(root=document){
  for(const title of root.querySelectorAll('.object-full-title')){
    const container=title.closest('.common-object,.notes-dialog');
    const heading=container?.querySelector(':scope > summary > .node-title,.history-dialog-header h2');
    title.hidden=!heading||(heading.dataset.truncated!=='true'&&heading.scrollHeight<=heading.clientHeight+1);
  }
}
window.addEventListener('resize',()=>syncObjectTitles());
