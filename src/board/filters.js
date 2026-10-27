// The owner, status and search filters of the board.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
import {notify} from 'board/feedback.js';
import {api,objectPart} from 'board/api.js';
import {personLabel} from 'board/format.js';
import {inputQueue} from 'board/object-types.js';
import {rootCollectionTabs} from 'board/collections.js';
import {commonObjectCard} from 'board/card.js';
import {resultFilters,valueSignal} from 'board/result-filter.js';
export function commonObjectFilters(root,contents,redraw){
  const controls=E('div',undefined,'filter-fields');let request=0;
  const picker=(label,options)=>{const select=E('select');select.setAttribute('aria-label',label);for(const [value,title] of options){const option=E('option',title);option.value=value;select.append(option);}controls.append(select);return select;};
  const owner=picker('Object owner',[['','All owners'],...Object.keys(state.data.state.members).map(id=>[id,personLabel(id)])]);
  const status=picker('Object status',[['','All statuses']]);const markStatus=statusFilter(status);
  resultFilters(controls,redraw);
  // The search box uses the server search of joppa_search and combines with the other filters.
  const search=E('input',undefined,'filter-search');search.type='search';search.placeholder='Search';search.setAttribute('aria-label','Search objects');controls.append(search);
  const update=async()=>{const mine=++request,generation=state.loadNumber;const selected={kind:state.data.activeRootKind,owner:owner.value,status:status.value,query:search.value.trim()};const filtering=Boolean(selected.owner||selected.status||selected.query);state.data.filterActive=filtering;contents.hidden=!filtering;const holder=E('div');contents.replaceChildren(holder);try{
    if(!filtering){rootCollectionTabs(root);return;}
    for(const panel of root.querySelectorAll('.root-collection'))panel.hidden=true;
    holder.append(E('p','Loading objects…','object-loading hint'));
    const params={parent:'*',limit:'100'};if(selected.kind)params.kind=selected.kind;if(inputQueue(selected.kind))params.note_status='unprocessed';let cursor;const items=[];
    do{const page=await objectPart(cursor?{...params,cursor}:params);if(mine!==request||generation!==state.loadNumber||!controls.isConnected)return;items.push(...page.items);cursor=page.next_cursor;}while(cursor);
    let found;
    if(selected.query){const reply=await api('workspaces/'+encodeURIComponent(state.ws)+'/search?'+new URLSearchParams({q:selected.query,limit:'50'}));if(mine!==request||generation!==state.loadNumber||!controls.isConnected)return;found=new Set(reply.items.map(item=>item.kind+':'+item.id));}
    const rows=items.filter(item=>(selected.kind||!inputQueue(item.kind))&&(!selected.owner||item.owner===selected.owner)&&(!selected.status||valueSignal(item)?.tone===selected.status)&&(!found||found.has(item.address)));
    holder.replaceChildren(...rows.map(commonObjectCard));if(!rows.length)holder.append(E('p','No objects match these filters.','hint'));
  }catch(e){if(mine===request)notify(e);}};
  state.data.filterActive=false;state.data.filterUpdate=update;
  owner.onchange=update;status.onchange=()=>{markStatus();update();};let typing;search.oninput=()=>{clearTimeout(typing);typing=setTimeout(update,250);};root.append(controls);return controls;
}
// The status filter lists the tones of the schema status list in its order, each with its dot.
// The closed filter shows the dot of the selected tone. A native list colours the option text.
const STATUS_LABELS={green:'Checks passed',red:'Checks failed',purple:'Dependency review',yellow:'Waiting or in progress',grey:'Unchecked or completed'};
function statusFilter(select){
  select.classList.add('status-filter');
  const listed=(Array.isArray(state.data.statuses)?state.data.statuses:[]).map(item=>item?.tone).filter(tone=>/^[a-z]+$/.test(tone||''));
  for(const tone of listed.length?listed:Object.keys(STATUS_LABELS)){
    const option=E('option',undefined,'status-option');option.value=tone;option.dataset.tone=tone;
    option.style.setProperty('--tone','var(--dot-'+tone+')');
    option.append(E('span',undefined,'status-circle '+tone),STATUS_LABELS[tone]||tone[0].toUpperCase()+tone.slice(1));select.append(option);
  }
  const mark=()=>{select.dataset.tone=select.value;select.style.setProperty('--tone',select.value?'var(--dot-'+select.value+')':'transparent');};
  mark();return mark;
}

