import javafx.application.Application;
import javafx.application.Platform;
import javafx.stage.Stage;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.GsonBuilder;
import com.tangluobo.tomato.module.connect.MarkdownEditorPane;
import org.fxmisc.richtext.GenericStyledArea;

/** Runs the original editor's private editing rules in an isolated JavaFX process. */
public class MarkdownEditGolden {
    static MarkdownEditorPane fixturePane;
    static Object call(Object pane,String name,Class<?>[] types,Object... args) throws Exception {
        Method method=MarkdownEditorPane.class.getDeclaredMethod(name,types);
        method.setAccessible(true);return method.invoke(pane,args);
    }
    static Map<String,Object> run(String action,String input,int start,int end,boolean shift,String replacement,int[] rect) throws Exception {
        var pane=fixturePane;
        Field field=MarkdownEditorPane.class.getDeclaredField("editor");field.setAccessible(true);
        GenericStyledArea editor=(GenericStyledArea)field.get(pane);
        editor.replaceText(input);
        editor.selectRange(start,end);
        boolean handled=true;
        switch(action) {
            case "indent" -> call(pane,"handleTab",new Class<?>[]{boolean.class},shift);
            case "enter" -> handled=(Boolean)call(pane,"handleEnter",new Class<?>[]{});
            case "rectangle" -> {
                String[] names={"rectStartRow","rectStartCol","rectEndRow","rectEndCol"};
                for(int i=0;i<4;i++){Field f=MarkdownEditorPane.class.getDeclaredField(names[i]);f.setAccessible(true);f.setInt(pane,rect[i]);}
                if(replacement.isEmpty())call(pane,"deleteRectSelection",new Class<?>[]{});
                else call(pane,"insertIntoRectSelection",new Class<?>[]{String.class},replacement);
            }
        }
        var result=new LinkedHashMap<String,Object>();
        result.put("action",action);result.put("input",input);result.put("start",start);result.put("end",end);
        result.put("shift",shift);result.put("replacement",replacement);result.put("rectangle",rect);
        result.put("handled",handled);result.put("output",editor.getText());
        result.put("selectionStart",editor.getSelection().getStart());result.put("selectionEnd",editor.getSelection().getEnd());
        return result;
    }
    public static class Probe extends Application {
    public void start(Stage stage) throws Exception {
        try {
            fixturePane=new MarkdownEditorPane("fixture.md","",(content,success,error)->success.run(),MarkdownEditorPane.createLocalResourceLoader("fixture.md"));
            var cases=new ArrayList<Map<String,Object>>();
            for(String input:List.of("- item","  09. [x] ok","> > text","- [ ] ","ordinary","2147483647. x","+ item\nremaining","\t* [X] done"))
                cases.add(run("enter",input,input.length(),input.length(),false,"",new int[0]));
            for(String input:List.of("中文\nnext","    中文","\n\nlast","  a\n    b\nlast")) {
                cases.add(run("indent",input,0,input.length(),false,"",new int[0]));
                cases.add(run("indent",input,0,input.length(),true,"",new int[0]));
                cases.add(run("indent",input,input.length(),input.length(),false,"",new int[0]));
                cases.add(run("indent",input,input.length(),input.length(),true,"",new int[0]));
            }
            for(String replacement:List.of("","中")) {
                cases.add(run("rectangle","abcd\nx\n\n最后一行\n",0,0,false,replacement,new int[]{3,3,0,1}));
                cases.add(run("rectangle","😀a!\n中文b!",0,0,false,replacement,new int[]{0,2,1,3}));
                cases.add(run("rectangle","ab\nc\n",0,0,false,replacement,new int[]{0,1,2,1}));
            }
            Files.writeString(Path.of(getParameters().getRaw().get(0)),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(cases));
            Platform.exit();System.exit(0);
        }catch(Throwable error){error.printStackTrace();System.exit(1);}
    }
    }
    public static void main(String[] args){Application.launch(Probe.class,args);}
}
