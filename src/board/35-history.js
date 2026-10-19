// History is a tab beside the root tabs. It lists open journal entries, oldest first,
// and with "Show processed too" it also lists processed entries with their mark. The
// server sets the page size. Load more reads the next page.
function historyPanel(root,existing){
  let panel=existing.find(node=>node.dataset.collectionKind==='@history');
  if(panel)return panel;
  panel=E('section',undefined,'root-collection history-collection');panel.dataset.collectionKind='@history';
  panel.append(E('header',undefined,'collection-heading'),E('div',undefined,'collection-items'));root.append(panel);
  collectionHeading(panel,'History','Changes of this workspace. An open entry waits until a member marks it processed.');
  const summary=panel.querySelector(':scope > header');
  const controls=E('div',undefined,'note-controls');
  const option=E('label',undefined,'note-all'),all=E('input');all.type='checkbox';all.setAttribute('aria-label','Show processed too');
  option.append(all,E('span','Show processed too'));controls.append(option);summary.append(controls);
  const items=panel.querySelector(':scope > .collection-items');
  let request=0,loaded=false;
  const page=async(cursor,mine)=>{
    const params=new URLSearchParams();if(all.checked)params.set('all','true');if(cursor)params.set('cursor',cursor);
    const reply=await api('workspaces/'+encodeURIComponent(ws)+'/history?'+params);
    if(mine!==request||!panel.isConnected)return;
    items.append(...reply.items.map(historyRow));
    if(!reply.next_cursor)return;
    const more=button('Load more',async()=>{
      more.disabled=true;
      try{await page(reply.next_cursor,mine);more.remove();}catch(error){more.disabled=false;throw error;}
    },'text-button');
    items.append(more);
  };
  const load=async()=>{
    const mine=++request;loaded=true;items.replaceChildren(E('p','Loading history…','object-loading hint'));
    try{
      await page(undefined,mine);
      if(mine!==request)return;
      items.querySelector(':scope > .object-loading')?.remove();
      if(!items.children.length)items.append(E('p',all.checked?'No changes yet.':'No open changes.','hint'));
    }catch(error){if(mine===request){loaded=false;items.replaceChildren();notify(error);}}
  };
  all.onchange=load;
  panel.collectionLoad=()=>{if(!loaded)load();};
  return panel;
}
// A History row looks like a board row: a dot, a title, a type label, the date and the state.
// The dot shows the current status of the changed object. An entry without an object has no dot.
// Its body holds the change reason, the actor, a link to the object and the mark.
function historyRow(item){
  const open=item.status==='open';
  const card=E('details',undefined,'common-object history-entry history-'+item.status);card.dataset.position=String(item.position);
  if(item.kind)applyObjectColor(card,item.kind);
  const summary=E('summary',undefined,'title-row');
  const text='#'+item.position+' '+(item.title||item.address||item.op||'Change');
  const title=item.signal?.tone?nodeTitle(text,item.signal):E('span',text,'node-title');
  const badge=E('span',String(item.change||item.op||'change').toUpperCase(),'type-label type-badge');
  const at=E('span',item.at?eventDate(item.at):'','history-at note-state');
  summary.append(title,badge,E('span',undefined,'count-slot'),at,E('span',open?'Open':'Processed','note-state'));
  card.append(summary);
  const body=E('div',undefined,'object-body');
  if(item.reason)body.append(E('p',item.reason,'history-reason'));
  body.append(E('p',(item.actor||'')+(item.at?' · '+eventDate(item.at):''),'history-actor hint'));
  if(item.address){
    const link=E('a',item.address+(item.revision?' · revision '+item.revision:''));
    link.href='/?workspace='+encodeURIComponent(ws)+'&object='+encodeURIComponent(item.address)+(item.revision?'&revision='+item.revision:'');
    const line=E('p',undefined,'history-object');line.append(link);body.append(line);
  }
  if(item.processed){
    const mark=E('div',undefined,'history-mark');
    mark.append(E('p','Processed by '+item.processed.actor+(item.processed.at?' · '+eventDate(item.processed.at):''),'hint'));
    if(item.processed.reason)mark.append(E('p',item.processed.reason));
    if(item.processed.nodes?.length){
      const nodes=E('p',undefined,'history-nodes');
      for(const node of item.processed.nodes){
        const link=E('a',node);link.href='/?workspace='+encodeURIComponent(ws)+'&object='+encodeURIComponent(node);nodes.append(link,' ');
      }
      mark.append(nodes);
    }
    body.append(mark);
  }
  card.append(body);
  return card;
}
