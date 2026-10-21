// The root collection tabs and the collection headings.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {notify,button} from 'board/feedback.js';
import {closeAccountMenu} from 'board/account-menu.js';
import {applyObjectColor} from 'board/object-types.js';
import {historyPanel} from 'board/history.js';
import {appendObjectChildren} from 'board/object-list.js';
export function rootCollectionTabs(root){
  if(!Object.keys(state.data.objectTypes||{}).length)return;
  const types=Object.entries(state.data.objectTypes||{}).filter(([,type])=>type.root)
    .sort(([a,left],[b,right])=>(left.index??Infinity)-(right.index??Infinity)||a.localeCompare(b));
  const existing=[...root.querySelectorAll('.root-collection')];
  for(const panel of existing)panel.hidden=true;
  if(!types.length){root.querySelector(':scope > .collection-tabs')?.remove();return;}
  const panels=types.map(([kind,type])=>{
    let panel=existing.find(node=>node.dataset.collectionKind===kind);
    if(!panel){
      panel=E('section',undefined,'root-collection');panel.dataset.collectionKind=kind;panel.dataset.collectionLazy='true';applyObjectColor(panel,kind);
      panel.append(E('header',undefined,'collection-heading'),E('div',undefined,'collection-items'));root.append(panel);
      collectionHeading(panel,type.title,type.description||'');
    }
    return panel;
  });
  panels.push(historyPanel(root,existing));
  let nav=root.querySelector(':scope > .collection-tabs');
  if(!nav){nav=E('div',undefined,'collection-tabs');nav.setAttribute('role','tablist');nav.setAttribute('aria-label','Root collections');root.prepend(nav);}
  nav.replaceChildren();nav.hidden=false;
  const key='joppa-collection:'+state.ws;
  let selected;
  try{selected=sessionStorage.getItem(key);}catch{}
  if(!panels.some(panel=>panel.dataset.collectionKind===selected))selected=panels[0]?.dataset.collectionKind;
  const activate=(kind,focus=false)=>{
    closeAccountMenu();
    selected=kind;try{sessionStorage.setItem(key,kind);}catch{}
    for(const panel of panels){
      const active=panel.dataset.collectionKind===kind;panel.hidden=!active||Boolean(state.data.filterActive);
      const tab=nav.querySelector('[aria-controls="'+panel.id+'"]');tab.setAttribute('aria-selected',String(active));tab.tabIndex=active?0:-1;
      if(active){
        state.data.activeRootKind=kind;
        if(focus)tab.focus();
        if(state.data.filterActive)continue;
        panel.collectionLoad?.();
        if(panel.dataset.collectionLazy==='true'){
          delete panel.dataset.collectionLazy;
          const contents=panel.querySelector(':scope > .collection-items')||panel;
          contents.append(E('p','Loading objects…','object-loading hint'));
          appendObjectChildren(contents,undefined,kind).catch(error=>{panel.dataset.collectionLazy='true';contents.replaceChildren();notify(error);});
        }
      }
    }
    if(state.data.filterActive)state.data.filterUpdate?.();
  };
  for(const panel of panels){
    const kind=panel.dataset.collectionKind,type=state.data.objectTypes[kind]||{title:'History'};
    panel.id='collection-'+kind;panel.setAttribute('role','tabpanel');panel.setAttribute('aria-labelledby','tab-'+kind);panel.tabIndex=-1;
    const tab=button(type.title,()=>activate(kind),'collection-tab');applyObjectColor(tab,kind);tab.id='tab-'+kind;tab.setAttribute('role','tab');tab.setAttribute('aria-controls',panel.id);nav.append(tab);
  }
  nav.onkeydown=event=>{
    const tabs=[...nav.children],current=tabs.indexOf(document.activeElement);let next;
    if(event.key==='ArrowRight')next=(current+1)%tabs.length;
    if(event.key==='ArrowLeft')next=(current+tabs.length-1)%tabs.length;
    if(event.key==='Home')next=0;if(event.key==='End')next=tabs.length-1;
    if(next!==undefined){event.preventDefault();activate(panels[next].dataset.collectionKind,true);}
  };
  activate(selected);
}
export function collectionHeading(section,title,description,label){
  const summary=section.querySelector(':scope > summary,:scope > header');
  const caption=E('span',undefined,'collection-caption');
  caption.append(E('span',title,'collection-title'),E('span',description,'collection-description'));
  summary.replaceChildren(caption);
  if(label)summary.append(E('span',label,'collection-label'));
}
