// The preview pane: a row that opens in the tree shows its document on the right. The tree
// keeps the children of the row, so a person moves through the tree and reads in the pane.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {notify} from 'board/feedback.js';
import {objectPart} from 'board/api.js';
import {reducedMotion} from 'board/motion.js';
import {applyObjectColor,objectTypeLabel} from 'board/object-types.js';
import {fillCommonObject} from 'board/object-detail.js';
// The pane opens only on a wide screen. A narrow screen keeps the document in the row.
const WIDE=window.matchMedia('(min-width: 900px)');
const EASE_OUT='cubic-bezier(.2,.75,.25,1)';
let shown=null,paneAnimation=null,loadingTimer=null;
function stopPaneAnimation(){
  clearTimeout(loadingTimer);loadingTimer=null;
  paneAnimation?.cancel();paneAnimation=null;
}
function buildCard(address,titleText,content){
  const card=E('div',undefined,'common-object preview-card');card.dataset.objectAddress=address;applyObjectColor(card,address.split(':')[0]);
  const head=E('div',undefined,'preview-head');
  const title=E('h2',titleText,'preview-title');
  const close=E('button','×','preview-close');close.type='button';close.title='Close preview';close.setAttribute('aria-label','Close preview');
  close.addEventListener('click',closePreview);
  head.append(objectTypeLabel(address.split(':')[0]),title,close);
  const body=E('div',undefined,'object-body');
  if(content)content(body);
  card.append(head,body);
  return card;
}
export function previewOpen(){return WIDE.matches;}
export async function showPreview(address){
  const pane=document.getElementById('preview');
  if(!pane||!WIDE.matches)return;
  const opening=pane.hidden||pane.classList.contains('is-closing');
  stopPaneAnimation();pane.classList.remove('is-closing');
  shown=address;mark(address);
  document.querySelector('.layout')?.classList.add('has-preview');pane.hidden=false;
  if(opening){
    pane.replaceChildren(buildCard(address,'',body=>body.append(E('p','Loading object…','object-loading hint'))));
    if(!reducedMotion()){
      paneAnimation=pane.animate([{opacity:0,transform:'translateX(18px)'},{opacity:1,transform:'none'}],{duration:260,easing:EASE_OUT});
      paneAnimation.onfinish=()=>{if(paneAnimation)paneAnimation=null;};
    }
  }else{
    pane.classList.add('is-switching');
    loadingTimer=setTimeout(()=>{
      if(shown===address)pane.replaceChildren(buildCard(address,'',body=>body.append(E('p','Loading object…','object-loading hint'))));
    },120);
  }
  const generation=state.loadNumber;
  try{
    const detail=await objectPart({address});
    if(generation!==state.loadNumber||shown!==address)return;
    clearTimeout(loadingTimer);loadingTimer=null;pane.classList.remove('is-switching');
    const card=buildCard(address,detail.document.data.title,body=>fillCommonObject(body,detail));
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
  const pane=document.getElementById('preview');
  if(pane){pane.classList.remove('is-closing','is-switching');pane.hidden=true;pane.replaceChildren();}
  document.querySelector('.layout')?.classList.remove('has-preview');
}
// The row of the shown document gets the selected look.
function mark(address){
  for(const row of document.querySelectorAll('#tree .common-object.previewed'))row.classList.remove('previewed');
  if(address)for(const row of document.querySelectorAll('#tree .common-object'))if(row.dataset.objectAddress===address)row.classList.add('previewed');
}
WIDE.addEventListener('change',()=>{if(!WIDE.matches)hidePreview();});
document.addEventListener('keydown',e=>{if(e.key==='Escape'&&shown)closePreview();});
