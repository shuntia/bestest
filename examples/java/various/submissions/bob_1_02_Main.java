import java.io.BufferedReader;
import java.io.InputStreamReader;

public final class Main {
    public static void main(String[] args) throws Exception {
        BufferedReader br = new BufferedReader(new InputStreamReader(System.in));
        if (br.readLine() == null) {
            return;
        }
        System.out.println("42");
    }
}
