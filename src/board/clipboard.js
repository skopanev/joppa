// The copy buttons that copy an ID for an agent or a link to a node.
import {E} from 'board/dom.js';
import {toast,button} from 'board/feedback.js';
// The texts of each copy button: the ready and the copied label, and the failure toast,
// dialog title, field label and hint.
const ID={ready:'Copy ID for agent',copied:'Copied ID for agent',failed:'Could not copy the ID. Select it in the dialog.',
  dialog:'Copy ID',field:'ID',hint:'Select the ID. Use your device copy command.'};
const LINK={ready:'Copy link to node',copied:'Copied link to node',failed:'Could not copy the link. Select it in the dialog.',
  dialog:'Copy link',field:'Link',hint:'Select the link. Use your device copy command.'};
/// One copy action for every card. The label changes only after the clipboard
/// actually accepted the text, and the button keeps its size so the card never
/// moves.
async function copyAction(trigger,text,labels){
  if(!text){toast('Nothing to copy yet. Reload the page.',true);return false;}
  let copied=false;
  try{
    if(!navigator.clipboard?.writeText)throw Error('Clipboard unavailable');
    await navigator.clipboard.writeText(text);
    copied=true;
  }catch{
    const field=E('textarea',text);field.style.position='fixed';field.style.opacity='0';field.style.top='0';
    document.body.append(field);field.select();
    try{copied=document.execCommand('copy');}catch{copied=false;}
    field.remove();
  }
  if(!copied){
    toast(labels.failed,true);
    const dialog=E('dialog',undefined,'copy-dialog');dialog.setAttribute('aria-label',labels.dialog);
    const field=E('input');field.value=text;field.readOnly=true;field.setAttribute('aria-label',labels.field);
    dialog.append(E('h2',labels.dialog),E('p',labels.hint),field,button('Close',()=>dialog.close()));
    dialog.addEventListener('close',()=>dialog.remove());document.body.append(dialog);dialog.showModal();field.focus();field.select();
    return false;
  }
  trigger.classList.add('copied');trigger.dataset.copyState='copied';
  trigger.setAttribute('aria-label',labels.copied);trigger.title=labels.copied;
  clearTimeout(trigger.copyTimer);
  trigger.copyTimer=setTimeout(()=>{if(trigger.isConnected){trigger.classList.remove('copied');trigger.dataset.copyState='ready';trigger.setAttribute('aria-label',labels.ready);trigger.title=labels.ready;}},3000);
  toast('Copied '+text);
  return true;
}
const ns='http://www.w3.org/2000/svg';
const icon=(className,width)=>{const svg=document.createElementNS(ns,'svg');for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width,height:width,fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true',focusable:'false'}))svg.setAttribute(key,value);svg.classList.add(className);return svg;};
const part=(tag,attributes)=>{const node=document.createElementNS(ns,tag);for(const [key,value] of Object.entries(attributes))node.setAttribute(key,value);return node;};
// A .copy button with its labels. The symbol shows until the copy, the check after it.
function copyControl(text,labels,symbol){
  const control=button('',()=>copyAction(control,text,labels),'copy');control.type='button';control.dataset.copyState='ready';
  control.title=labels.ready;control.setAttribute('aria-label',labels.ready);
  const check=icon('copy-check','14');check.append(part('path',{d:'m5 12 4 4L19 6'}));
  control.append(symbol,check);control.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();});return control;
}
export function copyButton(text){
  const copy=icon('copy-symbol','14');copy.append(part('rect',{x:'8',y:'8',width:'12',height:'12',rx:'2'}),part('path',{d:'M16 8V4a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h4'}));
  const control=copyControl(text,ID,copy);
  const face=icon('agent-face','18');
  for(const [tag,attributes] of [['rect',{x:'4',y:'7',width:'16',height:'13',rx:'4'}],['path',{d:'M12 7V4m-2 0h4M1 12v4m22-4v4M9 16h6'}],['circle',{cx:'8.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}],['circle',{cx:'15.5',cy:'12',r:'1',fill:'currentColor',stroke:'none'}]])face.append(part(tag,attributes));
  control.append(face);return control;
}
// The copy button of a link: a chain-link symbol in place of the copy symbol.
export function copyLinkButton(url){
  const chain=icon('copy-symbol','14');
  chain.append(part('path',{d:'M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7'}),part('path',{d:'M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7'}));
  return copyControl(url,LINK,chain);
}
