// Test results of acceptance criteria.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {api} from 'board/api.js';
import {statusCircle} from 'board/status.js';
import {when} from 'board/format.js';
import {objectSection} from 'board/object-detail.js';
import {valueStatus} from 'board/result-filter.js';
// A criterion takes its dot from its tests: green when every test passes,
// red when one fails, grey otherwise, and grey "No tests" without tests.
const testTone=status=>status==='pass'||status==='green'?'green':['fail','red'].includes(status)?'red':'grey';
function testSummary(s){
  const passed=s.tests.filter(t=>t.status==='pass').length;
  return 'Tests: '+passed+' of '+s.tests.length+' pass'+(s.commit?' · commit '+s.commit.slice(0,7):'')+(s.commits_behind?' · '+s.commits_behind+' commits behind':'')+(s.tests_draft?' · list needs saving':'');
}
export async function applyTestStatuses(items){
  const criteria=items.filter(item=>state.data.objectTypes?.[item.kind]?.role==='ac').map(item=>item.address);
  if(!criteria.length)return;
  const ready=new Set(items.filter(item=>item.signal?.label==='Ready for work').map(item=>item.address));
  let reply;
  try{reply=await api('workspaces/'+encodeURIComponent(state.ws)+'/tests?ac='+encodeURIComponent(criteria.join(',')));}catch{return;}
  for(const status of (reply.items||[reply]).map(valueStatus)){
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
export function criterionTestsSection(address){
  const group=objectSection('Tests','object-tests');
  api('workspaces/'+encodeURIComponent(state.ws)+'/tests?ac='+encodeURIComponent(address)).then(valueStatus).then(s=>{
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
