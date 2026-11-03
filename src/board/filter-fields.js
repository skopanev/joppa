// The filter drop-downs of the schema filter fields (`filters` of the tree schema). The board
// read gives the present values of each field. A choice is one part of the `data` node
// filter: {"tags":["swap"]} for an element of a list field, {"priority":"p0"} for a string.
// The parts of a link `data` filter that no drop-down shows stay in the filter as they are.
import {state} from 'board/state.js';
export function fieldPickers(picker,link){
  let rest=null;
  try{const parsed=link?JSON.parse(link):null;if(parsed&&typeof parsed==='object'&&!Array.isArray(parsed))rest=parsed;}catch{}
  const selects=[];
  for(const {field,values} of Array.isArray(state.data.filters)?state.data.filters:[]){
    if(typeof field!=='string'||!Array.isArray(values))continue;
    const options=values.map(({value,list})=>[JSON.stringify({[field]:list?[value]:value}),value]);
    const select=picker('data.'+field,'Filter '+field,[['','All '+field],...options]);
    select.classList.add('field-filter');select.dataset.field=field;
    // The drop-down takes the part of the link filter for its field.
    const wanted=rest&&field in rest?JSON.stringify({[field]:rest[field]}):null;
    if(wanted&&options.some(([value])=>value===wanted)){select.value=wanted;delete rest[field];}
    selects.push(select);
  }
  return {
    selects,
    // A link value that is no JSON object stays as it is, and the server refuses it.
    parsed:rest!==null||!link,
    rest:()=>rest&&Object.keys(rest).length?JSON.stringify(rest):'',
    data(){
      const merged=Object.assign({},rest||{},...selects.filter(s=>s.value).map(s=>JSON.parse(s.value)));
      return Object.keys(merged).length?JSON.stringify(merged):'';
    },
  };
}
