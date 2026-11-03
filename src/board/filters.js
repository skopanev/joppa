// The owner, assignee, status, readiness and search filters of the board. The server applies
// the node filters (src/node_filter.rs) at every level of the tree. A board link can carry
// them in its query, for example /?workspace=w&assignee=user-1&ready=false: each picker shows
// its link value, and a picker change writes its value back to the link.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {notify,button} from 'board/feedback.js';
import {api,objectPart,PAGE} from 'board/api.js';
import {personLabel} from 'board/format.js';
import {inputQueue} from 'board/object-types.js';
import {rootCollectionTabs} from 'board/collections.js';
import {commonObjectCard} from 'board/card.js';
import {searchScope,searchRows} from 'board/search-results.js';
export const LINK_FILTERS=['parent','depth','kind','status','owner','assignee','ready','data'];
export function commonObjectFilters(root,contents,apply){
  const controls=E('div',undefined,'filter-fields');let request=0;
  const query=new URLSearchParams(location.search);const linked=Object.fromEntries(LINK_FILTERS.map(k=>[k,query.get(k)]).filter(([,v])=>v));
  const picker=(name,label,options)=>{const select=E('select');select.name=name;select.setAttribute('aria-label',label);for(const [value,title] of options){const option=E('option',title);option.value=value;select.append(option);}controls.append(select);return select;};
  // A person of a link who is not a member, for example a former member, gets an option too.
  const people=(name,label,all)=>{const ids=Object.keys(state.data.state.members);if(linked[name]&&!ids.includes(linked[name]))ids.push(linked[name]);return picker(name,label,[['',all],...ids.map(id=>[id,personLabel(id)])]);};
  const owner=people('owner','Object owner','All owners'),assignee=people('assignee','Object assignee','All assignees');
  const status=picker('status','Object status',[['','All statuses']]);const markStatus=statusFilter(status);
  const ready=picker('ready','Object readiness',[['','All readiness'],['true','Ready'],['false','Not ready']]);
  const pickers=[owner,assignee,status,ready];
  // Each picker takes its link value. The other link filters show in one line.
  for(const select of pickers)if([...select.options].some(o=>o.value===linked[select.name])){select.value=linked[select.name];delete linked[select.name];}markStatus();
  const fromLink=E('p',Object.entries(linked).map(([k,v])=>k+' = '+v).join(' · '),'link-filters hint');fromLink.hidden=!Object.keys(linked).length;
  // The search box uses the server search of joppa_search and combines with the other filters.
  const search=E('input',undefined,'filter-search');search.type='search';search.placeholder='Search';search.setAttribute('aria-label','Search objects');controls.append(search);
  const scope=searchScope(controls,()=>update());
  // A graph gives `apply`: it reads the matches itself and dims the other nodes.
  const update=async()=>{const mine=++request,generation=state.loadNumber;const text=search.value.trim();
    const params={...linked,...Object.fromEntries(pickers.filter(s=>s.value).map(s=>[s.name,s.value]))};
    const filtering=Boolean(text||Object.keys(params).length);state.data.filterActive=filtering;
    const current=()=>mine===request&&generation===state.loadNumber&&controls.isConnected;
    if(apply)return apply({params,text,scope:scope(),filtering,current}).catch(e=>{if(mine===request)notify(e);});
    contents.hidden=!filtering;const holder=E('div');contents.replaceChildren(holder);try{
    if(!filtering){rootCollectionTabs(root);return;}
    for(const panel of root.querySelectorAll('.root-collection'))panel.hidden=true;
    holder.append(E('p','Loading objects…','object-loading hint'));
    const list=text?{...params,q:text,limit:PAGE,...scope()}:{parent:'*',limit:PAGE,...params};
    if(!text&&inputQueue(list.kind))list.note_status='unprocessed';
    // One page at a time: Load more adds the next page of the same filters.
    const show=async cursor=>{const page=await matches(text,cursor?{...list,cursor}:list);if(!current())return;
      holder.querySelector(':scope > .object-loading')?.remove();
      holder.append(...(text?searchRows(page.items):page.items.filter(item=>list.kind||!inputQueue(item.kind)).map(commonObjectCard)));
      if(!page.next_cursor&&!holder.querySelector('.common-object,.search-result'))holder.append(E('p','No objects match these filters.','hint'));
      if(page.next_cursor){const more=button('Load more',async()=>{more.disabled=true;try{await show(page.next_cursor);more.remove();}catch(e){more.disabled=false;notify(e);}},'text-button filter-more');holder.append(more);}};
    await show();
  }catch(e){if(mine===request)notify(e);}};
  // A picker change writes the picker values to the link and keeps the other query values.
  const relink=()=>{const next=new URLSearchParams(location.search);for(const select of pickers){if(select.value)next.set(select.name,select.value);else next.delete(select.name);}history.replaceState(history.state,'',location.pathname+'?'+next+location.hash);};
  state.data.filterActive=false;
  for(const select of pickers)select.onchange=()=>{markStatus();relink();update();};
  let typing;search.oninput=()=>{clearTimeout(typing);typing=setTimeout(update,250);};root.append(controls,fromLink);if(Object.keys(linked).length||pickers.some(s=>s.value))update();return controls;
}
// One page of the matches: the server search for a text, else the node list.
export function matches(text,params){
  return text?api('workspaces/'+encodeURIComponent(state.ws)+'/search?'+new URLSearchParams(params)):objectPart(params);
}
// The status filter lists the tones of the schema status list in its order, each with its dot.
// The board lists only present nodes, so the filter omits the tone of archived nodes.
// The server gives the label of each tone. The closed filter shows the dot of the selected
// tone. A native list colours the option text.
function statusFilter(select){
  select.classList.add('status-filter');
  const listed=(Array.isArray(state.data.statuses)?state.data.statuses:[]).filter(item=>/^[a-z]+$/.test(item?.tone||'')&&item.tone!==state.data.archived_tone);
  for(const {tone,label} of listed){
    const option=E('option',undefined,'status-option');option.value=tone;option.dataset.tone=tone;
    option.style.setProperty('--tone','var(--dot-'+tone+')');
    option.append(E('span',undefined,'status-circle '+tone),label||tone);select.append(option);
  }
  const mark=()=>{select.dataset.tone=select.value;select.style.setProperty('--tone',select.value?'var(--dot-'+select.value+')':'transparent');};
  mark();return mark;
}

