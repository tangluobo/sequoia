import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.Scene;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javafx.scene.control.ListView;
import javafx.stage.Popup;
import javax.imageio.ImageIO;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.GsonBuilder;
import com.tangluobo.tomato.module.connect.view.SqlEditorPane;
import org.fxmisc.richtext.InlineCssTextArea;

/** Snapshot only the original popup's own node, never the desktop. */
public class SqlCompletionWindowOracle {
    static Object field(Object target,String name)throws Exception {var f=target.getClass().getDeclaredField(name);f.setAccessible(true);return f.get(target);}
    public static class Probe extends Application {
        public void start(Stage stage)throws Exception {
            var pane=new SqlEditorPane();
            pane.setMetadataCompletions(List.of("main"),"main",List.of("main"),Map.of("Users",List.of("id","name")));
            stage.setScene(new Scene(pane,600,300));stage.setOpacity(0.01);stage.show();
            var editor=(InlineCssTextArea)field(pane,"textArea");
            editor.replaceText("Users.");editor.moveTo(6);editor.requestFocus();
            var wait=new PauseTransition(Duration.millis(1500));
            wait.setOnFinished(event->{try{
                var refresh=SqlEditorPane.class.getDeclaredMethod("refreshCompletionPopup");refresh.setAccessible(true);refresh.invoke(pane);
                var list=(ListView<?>)field(pane,"completionList");
                list.applyCss();list.layout();
                var path=Path.of(getParameters().getRaw().get(0));
                ImageIO.write(SwingFXUtils.fromFXImage(list.snapshot(null,null),null),"png",path.toFile());
                var info=new LinkedHashMap<String,Object>();info.put("width",list.getWidth());info.put("height",list.getHeight());info.put("rowHeight",list.getFixedCellSize());info.put("items",list.getItems());info.put("showing",((Popup)field(pane,"completionPopup")).isShowing());
                Files.writeString(Path.of(path.toString()+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(info));
                stage.close();Platform.exit();System.exit(0);
            }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
        }
    }
    public static void main(String[] args){Application.launch(Probe.class,args);}
}
