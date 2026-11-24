import com.google.gson.*;
import com.tangluobo.tomato.module.connect.markdown.syntaxhighlighter.IdeaCodeHighlighter;
import java.nio.file.*;
/** Pure tokenizer from the current original checkout, without a GUI or configured services. */
public class SqlHighlightGolden {
 public static void main(String[] args)throws Exception{
  var rows=new JsonArray();
  String[] inputs={"SELECT id,name FROM Users WHERE id>0","select count(*) from t group by x having sum(x)>1","Select f (1), g\t(2), h\n(3)","SELECT '中文🍅', \"column\", `table`","SELECT 1e-3, 2E+5, 0xff, 0b01, 3.14, .5, 1.","123abc 1__2 0x_ 0b_ 12f 12U 01 1_2","中文123 中select éfrom α123","SELECT 'a''b', 'a\\'b', \"a\\\"b\"","-- comment\nSELECT 1","// not a SQL comment\nSELECT 1","# not a SQL comment\nSELECT 1","/* block\ncomment */ SELECT 1","/* unclosed SELECT 'value'","'unclosed SELECT 1","@name @123 @@version @a_b $1 $fn(1)","SELECT `name\\`value` FROM `中文`","SELECT '\\z' FROM x","SELECT 'a\\\nb' FROM x","SELECT 'a\\\rb' FROM x","SELECT 'a\\\u0085b' FROM x","SELECT 'a\\\u2028b' FROM x","SELECT 'a\nb' FROM x","UPDATE t SET n=1; DELETE FROM t WHERE id=2","create table t(a int); alter table t add b text","a\u00a0(1) b\u2003(2) c\r(3)","SELECT 1 -- comment\r\nFROM t","SELECT true,false,null,returning,window,partition","λfunc(1) funcλ(2) _name$(3)","SELECT $$not-a-string$$, $tag$select$tag$","--\u2028SELECT 1\nFROM t","", "  \t\r\n"};
  for(String input:inputs){var row=new JsonObject();row.addProperty("input",input);var tokens=new JsonArray();for(var token:IdeaCodeHighlighter.tokenize(input,"sql")){var item=new JsonObject();item.addProperty("text",token.text());item.addProperty("kind",token.type().name());tokens.add(item);}row.add("tokens",tokens);rows.add(row);}
  Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(rows));
 }
}
