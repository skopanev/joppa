// The floating hint of a status dot or a type badge.
import {E} from 'board/dom.js';
let statusHint=null;
export function hideStatusHint(){if(statusHint)statusHint.hidden=true;}
export function showStatusHint(circle){
  if(!statusHint){statusHint=E('div',undefined,'status-hint');statusHint.hidden=true;document.body.append(statusHint);}
  statusHint.textContent=circle.dataset.hint;
  statusHint.hidden=false;
  const dot=circle.getBoundingClientRect(),box=statusHint.getBoundingClientRect();
  const left=Math.max(8,Math.min(dot.left+dot.width/2-box.width/2,window.innerWidth-box.width-8));
  const below=dot.bottom+10;
  const top=below+box.height<=window.innerHeight-8?below:Math.max(8,dot.top-box.height-10);
  statusHint.style.left=left+'px';statusHint.style.top=top+'px';
}
document.addEventListener('scroll',hideStatusHint,true);
