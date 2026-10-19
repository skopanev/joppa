// History is a tab beside the root tabs. It lists open journal entries, oldest first,
// and with All it also lists processed entries with their mark.
function historyPanel(root,existing){
  let panel=existing.find(node=>node.dataset.collectionKind==='@history');
  if(panel)return panel;
  panel=E('section',undefined,'root-collection history-collection');panel.dataset.collectionKind='@history';
  panel.append(E('header',undefined,'collection-heading'),E('div',undefined,'collection-items'));root.append(panel);
  collectionHeading(panel,'History','Changes of this workspace. An open entry waits until a member marks it processed.');
  const summary=panel.querySelector(':scope > header');
  const controls=E('div',undefined,'note-controls');
  const option=E('label',undefined,'note-all'),all=E('input');all.type='checkbox';all.setAttribute('aria-label','All history');
  option.append(all,E('span','All'));controls.append(option);summary.append(controls);
  const items=panel.querySelector(':scope > .collection-items');
  let request=0,loaded=false;
  const load=async()=>{
    const mine=++request;loaded=true;items.replaceChildren(E('p','Loading history…','object-loading hint'));
    try{
      const rows=[];let cursor;
      do{
        const params=new URLSearchParams({limit:'15'});if(all.checked)params.set('all','true');if(cursor)params.set('cursor',cursor);
        const page=await api('workspaces/'+encodeURIComponent(ws)+'/history?'+params);
        if(mine!==request||!panel.isConnected)return;
        rows.push(...page.items);cursor=page.next_cursor;
      }while(cursor&&rows.length<300);
      items.replaceChildren(...rows.map(historyRow));
      if(!rows.length)items.append(E('p',all.checked?'No changes yet.':'No open changes.','hint'));
    }catch(error){if(mine===request){loaded=false;items.replaceChildren();notify(error);}}
  };
  all.onchange=load;
  panel.collectionLoad=()=>{if(!loaded)load();};
  return panel;
}
function historyRow(item){
  const row=E('div',undefined,'history-entry history-'+item.status);row.dataset.position=String(item.position);
  const line=E('div',undefined,'history-line');
  line.append(E('span','#'+item.position,'history-position'),E('span',item.status,'history-status'),
    E('span',item.change||item.op||'','history-change'),E('span',item.title||item.address||'','history-title'),
    E('span',item.actor||'','history-actor'));
  row.append(line);
  if(item.reason)row.append(E('p',item.reason,'history-reason hint'));
  if(item.processed){
    row.append(E('p','Processed by '+item.processed.actor+': '+(item.processed.reason||''),'history-mark hint'));
    if(item.processed.nodes?.length){
      const nodes=E('p',undefined,'history-nodes');
      for(const node of item.processed.nodes){
        const link=E('a',node);link.href='/?workspace='+encodeURIComponent(ws)+'&object='+encodeURIComponent(node);nodes.append(link,' ');
      }
      row.append(nodes);
    }
  }
  return row;
}
