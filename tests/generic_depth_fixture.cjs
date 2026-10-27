// The fixture of tests/generic_depth_browser.cjs: the page, six configured levels, the rows,
// the object documents and the answers of the mocked server.
const {readFileSync}=require('node:fs');
const path=require('node:path');
const root=path.join(__dirname,'..');
const html=readFileSync(path.join(root,'src/board.html'),'utf8')
  .replace('/* BOARD_STYLE */',readFileSync(path.join(root,'src/board.css'),'utf8'))
  .replace('/* BRAND_STYLE */',readFileSync(path.join(root,'src/brand.css'),'utf8')).replace('/* BOARD_IMPORTS */',require('./board_script.cjs'));

const chain=[['portfolio','Portfolio','group'],['stream','Stream','group'],['epic','Epic','group'],
  ['outcome','Outcome','requirement'],['measure','Measure','ac'],['action','Action','task']];
const types={};
chain.forEach(([kind,title,role],i)=>{types[kind]={title,role,index:i*10,root:i===0,
  parents:i===0?[]:[chain[i-1][0]],title_size:[21,18,16,17,16,15][i]};});
types.intake={title:'Intake',role:'document', status:'none',index:70,root:true,parents:[],inbox:true,
  description:'Customer input to review.'};
const schema={workspace:'w',current_revision:3,schema:{revision:3,actor:'owner',
  at:'2026-10-06T09:00:00Z',reason:'Six configured levels',types}};

const address=i=>`${chain[i][0]}:${chain[i][0]}-1`;
const row=(i,links=[])=>({address:address(i),kind:chain[i][0],id:`${chain[i][0]}-1`,
  title:`${chain[i][1]} one`+(i===4?' — A long acceptance condition stays readable on a narrow screen':''),revision:1,present:true,owner:'owner',at:'2026-10-06T09:00:00Z',
  work_ready:true,review_required:false,item_id:`W-${chain[i][0].toUpperCase()}-1`,
  parent:i===0?null:address(i-1),links});
// The deepest action points at a root-level intake: a link across two roots.
const rows=[...chain.map((_,i)=>row(i)),
  {...row(0),address:'intake:in-1',kind:'intake',id:'in-1',title:'Customer input',
   parent:null,item_id:'W-O-9',links:[{target:address(5),relation:'related'},{target:'intake:in-2',relation:'related'}]},
  {...row(0),address:'intake:in-2',kind:'intake',id:'in-2',title:'Second input',
   parent:null,item_id:'W-O-10',links:[{target:'intake:in-1',relation:'related'}]}];
rows[0].verification_summary={total:2,passed:1};
rows[1].verification_summary={total:2,passed:0};
rows[4].signal={tone:'green',label:'Checks passed'};
rows.push({...row(4),address:'measure:measure-2',id:'measure-2',title:'Failed condition',item_id:'W-MEASURE-2',signal:{tone:'red',label:'Checks failed'}});
const find=a=>rows.find(r=>r.address===a);
const document=a=>({workspace:'w',address:a,kind:find(a).kind,id:find(a).id,item_id:find(a).item_id,
  current_revision:1,revision_count:1,work_ready:true,review_required:false,
  document:{revision:1,parent:find(a).parent,present:true,links:find(a).links,
    data:{title:find(a).title,owner:a===address(1)?'rita':a===address(5)?'build-runner':'owner',body:'Read the current contract before work. Keep the source and its saved revisions.'},actor:'owner',at:'2026-10-06T09:00:00Z',reason:'Create'},
  links:find(a).links.map(l=>({link:l,title:find(l.target).title,kind:find(l.target).kind,
    current_revision:1,present:true})),incoming:a==='intake:in-1'?[{source:'intake:in-2',title:'Second input',relation:'related'}]:[],annotations:{},process:a===address(3)?{req:'context',verification:'pass'}:undefined});

const board={workspace:'w',custom_tree:false,lazy:true,
    object_types:types,member_labels:{owner:'sergey.kopanev',rita:'Rita Petrov'},state:{title:'Example product',members:{owner:'human',rita:'human','build-runner':'agent'},domains:{},
    capabilities:{},requirements:{},consumers:{},notes:{},projects:{}},summary:{},signals:{},
    launch:{configured:false,ready:false,blockers:[]},counts:{},system_ids:{}};
// The objects list answer: the children of a parent, every object, or the linked objects.
const listed=url=>{const related=url.searchParams.get('related_to');return {items:rows.filter(r=>{
    const parent=url.searchParams.get('parent');
    const kind=url.searchParams.get('kind');
    if(kind&&r.kind!==kind)return false;
    // A related list holds only the objects actually linked to that address.
    if(related)return r.links.some(l=>l.target===related)
      ||(find(related)?.links||[]).some(l=>l.target===r.address);
    return parent==='*'?true:(parent||null)===r.parent;
}),next_cursor:null};};

module.exports={html,chain,types,schema,address,row,rows,find,document,board,listed};
