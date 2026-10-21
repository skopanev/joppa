// Status dots and their hint text.
import {E} from 'board/dom.js';
import {hideStatusHint,showStatusHint} from 'board/status-hint.js';
import {personLabel,eventDate} from 'board/format.js';
export function statusCircle(signal,number){
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
export function assessmentHint(a){
  if(!a)return '';
  if(a.state==='dependency')return '\nWaits for: '+a.target;
  if(a.state==='unassessed')return '\nReadiness: Not assessed';
  if(a.state==='waiting')return '\nQuestion: '+a.reason+'\nRecipient: '+personLabel(a.recipient||'Unassigned');
  return '\nReadiness: Ready';
}
export function nodeTitle(text,signal){
  const title=E('span',text,'node-title');title.prepend(statusCircle(signal));return title;
}
