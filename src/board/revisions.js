// The revision dialog of an object.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {notify,button} from 'board/feedback.js';
import {objectPart} from 'board/api.js';
import {applyObjectColor,objectTitlePreview} from 'board/object-types.js';
import {fillCommonObject} from 'board/object-detail.js';
export async function showObjectRevisions(address){
  const generation=state.loadNumber;
  const detail=await objectPart({address});if(generation!==state.loadNumber)return;
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
