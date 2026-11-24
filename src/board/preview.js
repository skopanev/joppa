// The preview pane: a row that opens in the tree shows its document on the right. The tree
// keeps the children of the row, so a person moves through the tree and reads in the pane.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {historyIcon} from 'board/icons.js';
import {notify,button} from 'board/feedback.js';
import {copyButton} from 'board/clipboard.js';
import {objectPart} from 'board/api.js';
import {reducedMotion} from 'board/motion.js';
import {personLabel} from 'board/format.js';
import {applyObjectColor,objectTypeLabel} from 'board/object-types.js';
import {fillCommonObject} from 'board/object-detail.js';
import {showObjectRevisions} from 'board/revisions.js';
// The pane opens only on a wide screen. A narrow screen keeps the document in the row.
const WIDE=window.matchMedia('(min-width: 900px)');
const EASE_OUT='cubic-bezier(.2,.75,.25,1)';
const STORAGE_KEY='joppa-preview-pct',DEFAULT_PCT=62,MIN_PCT=25,MAX_PCT=80,RESIZER_W=25;
let shown=null,paneAnimation=null,loadingTimer=null,currentPct=DEFAULT_PCT;
function clampPct(v){return Math.min(MAX_PCT,Math.max(MIN_PCT,Math.round(v*10)/10));}
function applyPct(pct,save){
  currentPct=clampPct(pct);
  const layout=document.querySelector('.layout'),resizer=document.getElementById('preview-resizer');
  if(layout)layout.style.setProperty('--preview-pct',String(currentPct));
  if(resizer)resizer.setAttribute('aria-valuenow',String(Math.round(currentPct)));
  if(save){try{localStorage.setItem(STORAGE_KEY,String(currentPct));}catch{}}
}
function initResizer(){
  try{const saved=parseFloat(localStorage.getItem(STORAGE_KEY));if(Number.isFinite(saved))currentPct=clampPct(saved);}catch{}
  const resizer=document.getElementById('preview-resizer');
  if(!resizer)return;
  resizer.setAttribute('aria-valuemin',String(MIN_PCT));resizer.setAttribute('aria-valuemax',String(MAX_PCT));
  applyPct(currentPct,false);
  let activeId=null;
  const finish=()=>{
    if(activeId===null)return;
    activeId=null;resizer.classList.remove('is-dragging');document.body.classList.remove('is-resizing-preview');
  };
  resizer.addEventListener('pointerdown',e=>{
    if(e.button!==0)return;
    e.preventDefault();activeId=e.pointerId;resizer.setPointerCapture(e.pointerId);
    resizer.classList.add('is-dragging');document.body.classList.add('is-resizing-preview');
  });
  resizer.addEventListener('pointermove',e=>{
    if(e.pointerId!==activeId)return;
    const layout=document.querySelector('.layout');if(!layout)return;
    const rect=layout.getBoundingClientRect(),usable=Math.max(1,rect.width-RESIZER_W);
    const treeW=e.clientX-rect.left-RESIZER_W/2;
    applyPct(((usable-treeW)/usable)*100,true);
  });
  resizer.addEventListener('pointerup',finish);
  resizer.addEventListener('pointercancel',finish);
  resizer.addEventListener('lostpointercapture',finish);
  resizer.addEventListener('dblclick',()=>{
    try{localStorage.removeItem(STORAGE_KEY);}catch{}
    applyPct(DEFAULT_PCT,false);
  });
  resizer.addEventListener('keydown',e=>{
    if(e.key==='ArrowLeft'){e.preventDefault();applyPct(currentPct+3,true);}
    else if(e.key==='ArrowRight'){e.preventDefault();applyPct(currentPct-3,true);}
  });
}
initResizer();
function stopPaneAnimation(){
  clearTimeout(loadingTimer);loadingTimer=null;
  paneAnimation?.cancel();paneAnimation=null;
}
function treeRow(address){
  for(const row of document.querySelectorAll('#tree .common-object'))if(row.dataset.objectAddress===address)return row;
  return null;
}
function buildCard(address,titleText,copyId,content){
  const card=E('div',undefined,'common-object preview-card');card.dataset.objectAddress=address;applyObjectColor(card,address.split(':')[0]);
  const head=E('div',undefined,'preview-head');
  const row=treeRow(address),dot=row?.querySelector(':scope > summary .status-circle');
  if(dot)head.append(dot.cloneNode(true));
  const title=E('h2',titleText,'preview-title');
  const actions=E('span',undefined,'item-actions preview-actions');
  const copy=copyButton(copyId||row?.dataset.copyId||address);
  const history=button('',()=>showObjectRevisions(address),'history-button');history.type='button';
  history.title='Revisions';history.setAttribute('aria-label','Revisions');history.append(historyIcon());
  const close=E('button','×','preview-close');close.type='button';close.title='Close preview';close.setAttribute('aria-label','Close preview');
  close.addEventListener('click',closePreview);
  actions.append(copy,history,close);
  head.append(objectTypeLabel(address.split(':')[0]),title,actions);
  const body=E('div',undefined,'object-body');
  if(content)content(body,row);
  card.append(head,body);
  return card;
}
function enrichMeta(body,detail,row){
  const meta=body.querySelector(':scope > .object-details > .object-meta');
  if(!meta)return;
  const assignee=detail.document?.data?.assignee;
  if(assignee)meta.append(E('span','Assignee: '+personLabel(assignee)));
  for(const chip of row?.querySelectorAll(':scope > summary > .verification-chip')||[]){
    meta.append(chip.cloneNode(true));
  }
}
export function previewOpen(){return WIDE.matches;}
export function previewActive(){
  const pane=document.getElementById('preview');
  return WIDE.matches&&Boolean(shown)&&Boolean(pane)&&!pane.hidden&&!pane.classList.contains('is-closing');
}
export async function showPreview(address){
  const pane=document.getElementById('preview');
  if(!pane||!WIDE.matches)return;
  const opening=pane.hidden||pane.classList.contains('is-closing');
  stopPaneAnimation();pane.classList.remove('is-closing');
  shown=address;mark(address);
  const resizer=document.getElementById('preview-resizer');
  document.querySelector('.layout')?.classList.add('has-preview');pane.hidden=false;if(resizer)resizer.hidden=false;
  if(opening){
    pane.replaceChildren(buildCard(address,'',null,body=>body.append(E('p','Loading object…','object-loading hint'))));
    if(!reducedMotion()){
      paneAnimation=pane.animate([{opacity:0,transform:'translateX(18px)'},{opacity:1,transform:'none'}],{duration:260,easing:EASE_OUT});
      paneAnimation.onfinish=()=>{if(paneAnimation)paneAnimation=null;};
    }
  }else{
    pane.classList.add('is-switching');
    loadingTimer=setTimeout(()=>{
      if(shown===address)pane.replaceChildren(buildCard(address,'',null,body=>body.append(E('p','Loading object…','object-loading hint'))));
    },120);
  }
  const generation=state.loadNumber;
  try{
    const detail=await objectPart({address});
    if(generation!==state.loadNumber||shown!==address)return;
    clearTimeout(loadingTimer);loadingTimer=null;pane.classList.remove('is-switching');
    const card=buildCard(address,detail.document.data.title,detail.item_id,(body,row)=>{
      fillCommonObject(body,detail);
      enrichMeta(body,detail,row);
    });
    pane.replaceChildren(card);pane.scrollTop=0;
    if(!opening&&!reducedMotion()){
      card.animate([{opacity:0,transform:'translateY(6px)'},{opacity:1,transform:'none'}],{duration:210,easing:EASE_OUT});
    }
  }catch(e){
    if(shown===address){clearTimeout(loadingTimer);pane.classList.remove('is-switching');pane.replaceChildren();notify(e);}
  }
}
export function closePreview(){
  const pane=document.getElementById('preview');
  if(!pane||pane.hidden)return;
  shown=null;mark(null);stopPaneAnimation();pane.classList.remove('is-switching');
  if(reducedMotion()){hidePreview();return;}
  pane.classList.add('is-closing');
  const animation=pane.animate([{opacity:1,transform:'none'},{opacity:0,transform:'translateX(14px)'}],{duration:180,easing:'cubic-bezier(.4,0,.2,1)'});
  paneAnimation=animation;
  animation.onfinish=()=>{
    if(paneAnimation!==animation)return;
    paneAnimation=null;
    if(!shown)hidePreview();
  };
}
export function hidePreview(){
  stopPaneAnimation();shown=null;mark(null);
  const pane=document.getElementById('preview'),resizer=document.getElementById('preview-resizer');
  if(pane){pane.classList.remove('is-closing','is-switching');pane.hidden=true;pane.replaceChildren();}
  if(resizer){resizer.hidden=true;resizer.classList.remove('is-dragging');}
  document.body.classList.remove('is-resizing-preview');
  document.querySelector('.layout')?.classList.remove('has-preview');
}
// The row of the shown document gets the selected look.
function mark(address){
  for(const row of document.querySelectorAll('#tree .common-object.previewed'))row.classList.remove('previewed');
  if(address)for(const row of document.querySelectorAll('#tree .common-object'))if(row.dataset.objectAddress===address)row.classList.add('previewed');
}
WIDE.addEventListener('change',()=>{if(!WIDE.matches)hidePreview();});
document.addEventListener('keydown',e=>{if(e.key==='Escape'&&shown)closePreview();});
