package com.tangluobo.tomato.module.connect.view;
import java.nio.file.*;
import com.google.gson.*;
/** Invoke the unchanged original formatter without running its GUI/database constructor. */
public class SqlFormatGolden {
 public static void main(String[] args)throws Exception {
  var unsafeClass=Class.forName("sun.misc.Unsafe");var field=unsafeClass.getDeclaredField("theUnsafe");field.setAccessible(true);var unsafe=field.get(null);
  var instance=unsafeClass.getMethod("allocateInstance",Class.class).invoke(unsafe,SqlEditorView.class);
  var method=SqlEditorView.class.getDeclaredMethod("formatSql",String.class);method.setAccessible(true);
  var rows=new JsonArray();
  for(String sql:new String[]{""," ","select * from users where id=1 order by id limit 5","SELECT * FROM users WHERE id=1","Select * From users Where id=1","\tSELECT\n*\r\nFROM\tusers\nWHERE id=1","select ' from a where b ' as text from t","select a,b from t left join z on t.id=z.id where z.x=1 group by a having a>1 order by b limit 1 offset 2","insert into t values (1); update t set x=2 where x=1; delete from t where x=2","create table t(x int); drop table t; alter table t add x int","SELECT\u00a0*\u00a0FROM t","SELECT\u2003*\u2003FROM t","SELECT\u000b*\u000cFROM t","SELECT *\n\nFROM t\n\n WHERE id=1","select a from t union select b from z","-- FROM comment\nSELECT * FROM table1","SELECT '中文🍅' FROM 中文表 WHERE 名称='a'","SELECT * FROM t INNER JOIN z ON x=y","select * from t right join z on x=y cross join b full join c","\u0000 SELECT * FROM t \u0000"}){
   var row=new JsonObject();row.addProperty("input",sql);row.addProperty("output",(String)method.invoke(instance,sql));rows.add(row);
  }
  Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(rows));
 }
}
