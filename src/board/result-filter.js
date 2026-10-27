// The board filter by one value of a result field that the tree schema marks for the board.
// The server stores the tone of each node for each value. With a value selected, each row
// shows the tone for that value. Without a selection, each row shows its own tone.
import {state} from 'board/state.js';
import {E} from 'board/dom.js';
export function valueSignal(item){
  const key=state.resultValue;const tone=key&&item.signal?.tone?item.value_tones?.[key]:undefined;
  return tone?{...item.signal,tone,label:item.signal.label+' ('+key.replace(':',' = ')+')'}:item.signal;
}
// The test status of a criterion for the selected value, or its own status.
export function valueStatus(status){
  const key=state.resultValue||'',cut=key.indexOf(':'),field=key.slice(0,cut),value=key.slice(cut+1);
  const one=cut>0&&status?.values?.[field]?.[value];
  return one?{...one,address:status.address}:status;
}
export function resultFilters(controls,redraw){
  const filters=Array.isArray(state.data.result_filters)?state.data.result_filters:[];
  const selects=[];
  for(const filter of filters){
    const select=E('select');select.setAttribute('aria-label','Result '+filter.title);select.classList.add('result-filter');
    const all=E('option','All '+filter.title);all.value='';select.append(all);
    for(const value of filter.values||[]){const option=E('option',filter.title+': '+value);option.value=filter.field+':'+value;select.append(option);}
    if([...select.options].some(o=>o.value&&o.value===state.resultValue))select.value=state.resultValue;
    select.onchange=()=>{state.resultValue=select.value;for(const other of selects)if(other!==select)other.value='';redraw();};
    selects.push(select);controls.append(select);
  }
  // A selection that the schema no longer offers is cleared.
  if(state.resultValue&&!selects.some(s=>s.value))state.resultValue='';
}
