import com.tangluobo.tomato.module.connect.view.SqlCompletionEngine;
import com.google.gson.GsonBuilder;
import java.nio.file.*;
import java.util.*;
import java.util.regex.*;

/** Pure original completion engine; no GUI, saved hosts, database or clipboard. */
public class SqlCompletionGolden {
    public static void main(String[] args) throws Exception {
        String source=Files.readString(Path.of(args[0]));
        Matcher block=Pattern.compile("KEYWORDS = \\{(.*?)\\};",Pattern.DOTALL).matcher(source);
        if(!block.find())throw new IllegalStateException("Original keyword array missing");
        Matcher words=Pattern.compile("\"([A-Z_]+)\"").matcher(block.group(1));
        var items=new ArrayList<SqlCompletionEngine.Candidate>();
        while(words.find())items.add(new SqlCompletionEngine.Candidate(words.group(1),"关键字"));
        var databases=List.of("main","other","SELECT");
        var namespaces=List.of("public","main");
        var tables=new LinkedHashMap<String,List<String>>();
        tables.put("Users",List.of("id","Name","User_Name","Shared","σ","ς","İ","i","ı","ſ","s","²name","Ⅻname","١name","\u0345x","ΟΣ","Οσ"));
        tables.put("orders",List.of("ID","amount","shared"));
        tables.put("中文表",List.of("字段一","字段二"));
        tables.put("userEmpty",List.of());tables.put("select",List.of("name"));tables.put("main",List.of("main_field"));
        for(int i=149;i>=0;i--)tables.put(String.format("item_%03d",i),List.of());
        for(String database:databases)items.add(new SqlCompletionEngine.Candidate(database,"数据库"));
        for(String table:tables.keySet())items.add(new SqlCompletionEngine.Candidate(table,"表"));
        for(var columns:tables.values())for(String column:columns)items.add(new SqlCompletionEngine.Candidate(column,"字段"));
        var cases=new ArrayList<Map<String,Object>>();
        for(String sql:List.of("","sel","SELECT","s","main.","MAIN.us","public.","`Users`.n","\"Users\".i","[Users].","public.Users.s","unrelated.Users.","unknown.x","public.other.Users.","中文表.字段","Users..x",".x","(","SELECT 名字","a$","𝒜","a𝒜","u\u0345","²","Ⅻ","١","Users.σ","Users.ς","Users.İ","Users.ı","Users.s","Users.ſ","Users.Ο","[public].[Users].","SELECT \"public\".\"Users\".N","user","item_","public.item_","Users.Name","select.n","main.ma","other.")) {
            var completion=SqlCompletionEngine.complete(sql,sql.length(),items,"main",namespaces,tables);
            var result=new LinkedHashMap<String,Object>();result.put("sql",sql);result.put("caret",sql.length());result.put("start",completion.replacementStart());result.put("candidates",completion.candidates());cases.add(result);
        }
        var metadata=new LinkedHashMap<String,Object>();metadata.put("schema","public");metadata.put("database","main");metadata.put("databases",databases);metadata.put("namespaces",namespaces);
        var tableList=new ArrayList<Map<String,Object>>();for(var table:tables.entrySet())tableList.add(Map.of("name",table.getKey(),"columns",table.getValue()));metadata.put("tables",tableList);
        Files.writeString(Path.of(args[1]),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(Map.of("metadata",metadata,"cases",cases)));
        System.out.println("Original SQL completion cases: "+cases.size());
    }
}
