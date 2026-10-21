// Facts from the tree schema about object types: colour, title length, badge, inbox and verification.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {hideStatusHint,showStatusHint} from 'board/status-hint.js';
export function applyObjectColor(node,kind){
  const type=state.data.objectTypes?.[kind]||{};
  const color=type.color||'#63756b';
  if(/^#[0-9a-f]{6}$/i.test(color))node.style.setProperty('--object-color',color);
  const size=type.title_size;
  node.style.setProperty('--object-title-size',(Number.isInteger(size)&&size>=12&&size<=48?size:18)+'px');
}
export function objectTitlePreview(title,kind){
  const limit=state.data.objectTypes?.[kind]?.title_preview_length||64,characters=Array.from(title);
  return characters.length>limit?characters.slice(0,limit-1).join('').trimEnd()+'…':title;
}
// A type badge is a plain label: a click goes to its row, and no popover opens.
export function objectTypeLabel(kind){
  const type=state.data.objectTypes?.[kind]||{},title=type.title||kind.toUpperCase();
  const words=title.trim().split(/\s+/);
  const short=words.length===1&&title===title.toLocaleUpperCase()?title:words.map(word=>Array.from(word)[0]).join('').toLocaleUpperCase();
  const question=type.question||'',hint=question?title+'\n'+question:title;
  const label=E('abbr',type.badge||short,'type-label type-badge');
  label.dataset.hint=hint;label.title=hint;label.setAttribute('aria-label',title);
  label.addEventListener('pointerenter',()=>showStatusHint(label));
  label.addEventListener('pointerleave',hideStatusHint);
  return label;
}
/// Every configured inbox type gets its own collection, in schema order.
export function inboxKinds(){
  return Object.entries(state.data.objectTypes||{}).filter(([,t])=>t.inbox===true)
    .sort((a,b)=>(a[1].index??Infinity)-(b[1].index??Infinity)||a[0].localeCompare(b[0]))
    .map(([kind])=>kind);
}
/// An input queue shows unprocessed entries first. The schema declares it.
export function inputQueue(kind){
  return kind?state.data.objectTypes[kind]?.inbox===true:false;
}
// The schema selects the next types that can contain verification criteria.
export function verificationChildTitle(kind){
  const types=state.data.objectTypes,branches=new Set(Object.keys(types).filter(k=>types[k].role==='ac'));
  let changed=true;
  while(changed){const size=branches.size;for(const child of [...branches])for(const parent of types[child]?.parents||[])if(parent==='*')Object.keys(types).forEach(k=>branches.add(k));else branches.add(parent);changed=branches.size!==size;}
  return [...new Set(Object.entries(types).filter(([k,t])=>branches.has(k)&&(t.parents||[]).some(p=>p===kind||p==='*')).map(([,t])=>t.title))].join(' / ');
}
